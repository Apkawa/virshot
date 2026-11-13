//! Host-loop overlay backend: overlays painted on the HOST's egui event loop.
//!
//! winit 0.30 allows exactly one `EventLoop` per process, so the thread backend
//! in `service` owns that loop and cannot coexist with a host app that already
//! runs one (virshot runs eframe on the main thread). Inside a host app the
//! overlay is instead a child egui viewport of the host loop: the host calls
//! [`OverlayHost::update`] once per pass, and each live overlay is registered
//! with `Context::show_viewport_deferred`, which eframe turns into a real
//! transparent, decoration-less, always-on-top window. The host must run a
//! multi-viewport-capable backend: with `Context::embed_viewports` (the egui
//! default, and what a non-desktop backend keeps) the overlay is embedded in the
//! host window instead of becoming its own window.
//!
//! Coordinates here are **PHYSICAL desktop pixels**, converted to egui logical
//! points inside this module: egui multiplies viewport position and inner size
//! by the pixels-per-point of the window being moved (`egui-winit-0.37.0/src/
//! lib.rs`, `ViewportCommand::OuterPosition` / `InnerSize`), and for a child
//! viewport that is the CHILD window's ppp — which depends on the monitor the
//! window lands on, so a rect in the root's point space would be wrong on any
//! other monitor or DPI. Both backends therefore take physical rects; the
//! thread backend needs no conversion, this one divides (`logical_rect`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::content::{ImageCache, OverlayContent, paint};
use crate::error::OverlayError;
use crate::placement::{MonitorRect, Rect};
use crate::{OverlayId, OverlayWindowSpec};

/// Steady-state repaint cadence for overlay chips painted on the host loop.
///
/// A chip whose content, placement and visibility are unchanged needs no GPU work
/// per host pass, so `paint` schedules its next repaint after this interval
/// (~5 fps) instead of demanding one every pass — a toast sitting in `Hold` used
/// to spin the host loop at full rate. Event-driven wakes stay immediate
/// (`drain`: spawn, redraw/fade ticks; placement/DPI correction, first paint),
/// so animation and window changes never inherit this delay. egui keeps the
/// EARLIEST scheduled repaint (strict `delay < repaint_delay`), so a delayed
/// request yields exactly one repaint: `paint` re-issues it each pass to keep a
/// steady chip alive.
const REPAINT_INTERVAL: Duration = Duration::from_millis(200);

/// A command for the host loop, queued by [`OverlayHostHandle`] and drained by
/// [`OverlayHost::update`] on the host thread.
///
/// Deliberately not `service::LoopEvent`: that enum carries a readiness
/// handshake sender because the thread backend blocks until the window exists.
/// The host backend never blocks, so it needs no ack.
pub(crate) enum OverlayCommand {
    /// Register a new overlay viewport with this spec, content, and fade alpha.
    Spawn {
        id: OverlayId,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    },
    /// Replace the content and fade alpha of a registered overlay.
    Redraw {
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    },
    /// Close a registered overlay. Unknown ids are ignored.
    Hide { id: OverlayId },
    /// Close every overlay viewport.
    QuitAll,
}

/// Facade state shared by the host and every [`OverlayHostHandle`].
struct SharedState {
    sender: mpsc::Sender<OverlayCommand>,
    /// Ids the facade considers live: `redraw` on any other id is
    /// `OverlayError::UnknownWindow`.
    live: HashSet<OverlayId>,
    next_id: AtomicU64,
}

/// Content and texture state a deferred viewport callback reads each paint.
///
/// egui stores the callback as `Arc<dyn Fn + Send + Sync>`, so the painted data
/// lives behind `Arc`; the callback and the host thread run on the same thread,
/// so the mutexes are uncontended and only satisfy the `Sync` bound.
struct PaintState {
    content: Mutex<OverlayContent>,
    images: Mutex<ImageCache>,
    /// Fade alpha, stored as bits so a repaint can read it without a lock.
    alpha: AtomicU32,
    /// Set by the callback: the window has real pixels, so it may be shown.
    painted: AtomicBool,
    /// `ViewportCommand::ContentProtected` is a window attribute, sent once.
    protection_sent: AtomicBool,
    /// Set once `ViewportCommand::Visible(true)` has been sent: the chip is on
    /// screen, so the show sequence is not re-sent, because every
    /// `Context::send_viewport_cmd_to` pins this viewport's repaint delay to ZERO
    /// and deduplicates the delayed request of the same pass (the throttle needs
    /// commands only on a value change).
    shown: AtomicBool,
}

/// One live overlay: its spec plus the paint state the viewport shares.
struct HostWindow {
    spec: OverlayWindowSpec,
    viewport: egui::ViewportId,
    paint: Arc<PaintState>,
    /// Pixels-per-point of the last position/size correction sent to this
    /// window; `None` until the viewport reports one (`ViewportInfo::
    /// native_pixels_per_point`), so the builder's guess still stands.
    applied_ppp: Option<f32>,
}

impl HostWindow {
    fn new(id: OverlayId, spec: OverlayWindowSpec, content: OverlayContent, alpha: f32) -> Self {
        Self {
            spec,
            viewport: viewport_id(id),
            paint: Arc::new(PaintState {
                content: Mutex::new(content),
                images: Mutex::new(ImageCache::new()),
                alpha: AtomicU32::new(alpha.to_bits()),
                painted: AtomicBool::new(false),
                protection_sent: AtomicBool::new(false),
                shown: AtomicBool::new(false),
            }),
            applied_ppp: None,
        }
    }
}

/// One live window's data for a single paint pass, snapshotted out of
/// `self.windows` so the viewport callback never borrows the map.
struct LiveWindow {
    id: OverlayId,
    viewport: egui::ViewportId,
    spec: OverlayWindowSpec,
    paint: Arc<PaintState>,
    applied_ppp: Option<f32>,
}

/// Overlay windows painted on the host application's egui loop.
///
/// The host owns one instance and calls [`OverlayHost::update`] once per pass;
/// other threads talk to it through [`OverlayHost::handle`].
pub struct OverlayHost {
    shared: Arc<Mutex<SharedState>>,
    receiver: mpsc::Receiver<OverlayCommand>,
    windows: HashMap<OverlayId, HostWindow>,
    monitors: Mutex<Vec<MonitorRect>>,
    /// Placement diagnostics on stderr (`set_debug`). Off by default: every
    /// line is gated on this flag, so an ordinary host pays nothing.
    debug: AtomicBool,
    /// Kept so `Drop` can close the viewports it still owns.
    ctx: Option<egui::Context>,
}

impl OverlayHost {
    /// Create the host backend: the command channel and the window-state map.
    /// Nothing is created until `update` runs on the host thread.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            shared: Arc::new(Mutex::new(SharedState {
                sender,
                live: HashSet::new(),
                next_id: AtomicU64::new(1),
            })),
            receiver,
            windows: HashMap::new(),
            monitors: Mutex::new(Vec::new()),
            debug: AtomicBool::new(false),
            ctx: None,
        }
    }

    /// A `Send + Sync` facade for other threads (capture threads, toast timers).
    pub fn handle(&self) -> OverlayHostHandle {
        OverlayHostHandle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Monitors for placement (Stage 3's toast asks this for corner anchors).
    /// In this backend the host supplies them — physical monitor rects with the
    /// monitor's DPI scale attached, or synthetic rects in an example.
    pub fn monitors(&self) -> Vec<MonitorRect> {
        let monitors = self
            .monitors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        monitors.clone()
    }

    /// Replace the monitor rects.
    pub fn set_monitors(&self, monitors: Vec<MonitorRect>) {
        *self
            .monitors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = monitors;
    }

    /// Print placement diagnostics to stderr (`overlay: …` lines): one per
    /// spawn, one per DPI correction, and one per pass whose OBSERVED geometry
    /// differs from the requested physical rect by at least a pixel.
    ///
    /// A Windows rig reads these lines to tell a PLACEMENT defect (the window
    /// landed elsewhere or got the wrong size — 🅱) from a PRESENTATION defect
    /// (the window is where and how it was asked but shows nothing — 🅲): with
    /// the flag on, spawn + correction lines prove the backend asked for the
    /// rect, and the absence of a drift line proves the OS put the window
    /// there. Read once per pass, so enabling it mid-run takes effect at once.
    pub fn set_debug(&self, enabled: bool) {
        self.debug.store(enabled, Ordering::Relaxed);
    }

    /// Drain the queued commands and register every live overlay as a child
    /// viewport of the host loop.
    ///
    /// Must be called from the host thread inside the host's paint pass (egui
    /// needs `show_viewport_deferred` on every pass the child exists). Commands
    /// are applied in arrival order, so the last `Redraw` for an id wins; the
    /// viewport callback reads the shared paint state, so a later command is
    /// picked up by the next paint.
    ///
    /// The backend owns no timers: it paints the latest state it was given.
    /// Fade timing belongs to the caller (Stage 3's toast).
    pub fn update(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
        self.drain(ctx);
        self.paint(ctx);
    }

    /// Applies queued commands. `Close` is only sent to a viewport that already
    /// has a window; a viewport not shown this pass is closed by eframe's
    /// `remove_viewports_not_in` (`glow_integration.rs`).
    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(command) = self.receiver.try_recv() {
            match command {
                OverlayCommand::Spawn {
                    id,
                    spec,
                    content,
                    alpha,
                } => {
                    self.windows
                        .insert(id, HostWindow::new(id, spec, content, alpha));
                    if self.debug.load(Ordering::Relaxed) {
                        // The builder's numbers are what egui creates the window
                        // with: the physical rect divided by the target
                        // monitor's scale (`paint` uses the same conversion).
                        let (min, size) = logical_rect(spec.rect, spec.scale);
                        eprintln!(
                            "overlay: spawn id={id} scale={:.2} physical=({:.0},{:.0},{:.0},{:.0}) builder_logical=({:.1},{:.1},{:.1},{:.1})",
                            spec.scale,
                            spec.rect.x,
                            spec.rect.y,
                            spec.rect.w,
                            spec.rect.h,
                            min.x,
                            min.y,
                            size.x,
                            size.y,
                        );
                    }
                    // Never `request_repaint_of` a viewport without a window: it
                    // pins that viewport's `repaint_delay` to ZERO, so every later
                    // request is deduplicated away and the window never gets a
                    // redraw. The host loop repaint is enough —
                    // `paint` registers the viewport on this very pass.
                    ctx.request_repaint();
                }
                OverlayCommand::Redraw { id, content, alpha } => {
                    let Some(window) = self.windows.get_mut(&id) else {
                        continue;
                    };
                    let mut guard = window
                        .paint
                        .content
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    *guard = content;
                    window
                        .paint
                        .alpha
                        .store(alpha.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                    // A repaint of the root loop does NOT wake a Deferred child
                    // viewport, so a fade tick must wake the chip's own window —
                    // otherwise the 25 ms fade animation stalls. Same window-less
                    // hazard as in `Spawn`: `request_repaint_of` on a viewport with
                    // no window pins `repaint_delay` to ZERO and deduplicates every
                    // later request, so it is guarded by the window being present.
                    if ctx.input(|input| input.raw.viewports.contains_key(&window.viewport)) {
                        ctx.request_repaint_of(window.viewport);
                    }
                    // The host loop wakes too, so `paint` re-registers the chip on
                    // this pass and keeps the steady throttle re-issued.
                    ctx.request_repaint();
                }
                OverlayCommand::Hide { id } => {
                    self.close(id, ctx);
                }
                OverlayCommand::QuitAll => {
                    let ids: Vec<OverlayId> = self.windows.keys().copied().collect();
                    for id in ids {
                        self.close(id, ctx);
                    }
                }
            }
        }
    }

    /// Registers each live overlay with egui and drives the first-frame
    /// sequencing ported from `virshot`'s settings-window app
    /// (`src/gui/app.rs`): the window is created
    /// hidden, painted while hidden, and shown only after its callback ran.
    fn paint(&mut self, ctx: &egui::Context) {
        let max_texture_side = ctx.input(|input| input.max_texture_side);
        // Snapshot: egui stores the viewport callback as `Arc<dyn Fn + 'static>`,
        // so nothing here may borrow `self.windows` past this method.
        let live: Vec<LiveWindow> = self
            .windows
            .iter()
            .map(|(id, window)| LiveWindow {
                id: *id,
                viewport: window.viewport,
                spec: window.spec,
                paint: Arc::clone(&window.paint),
                applied_ppp: window.applied_ppp,
            })
            .collect();
        let mut closing = Vec::new();
        let has_live = !live.is_empty();

        for live_window in live {
            let LiveWindow {
                id,
                viewport,
                spec,
                paint: paint_state,
                applied_ppp,
            } = live_window;
            // A viewport the OS/Wayland already closed must not be re-registered.
            if ctx.input(|input| {
                input
                    .raw
                    .viewports
                    .get(&viewport)
                    .is_some_and(|info| info.close_requested())
            }) {
                closing.push(id);
                continue;
            }

            // egui needs `show_viewport_deferred` on EVERY pass the child
            // exists: the pass that registers it is the pass that
            // creates the window (`initialize_all_windows`).
            //
            // The builder is multiplied by the pixels-per-point eframe has when
            // it creates the window (`egui-winit` `pixels_per_point`), which is
            // derived from the monitor winit picks — unknowable here. The
            // target monitor's scale (`spec.scale`) is the best guess available;
            // the observed ppp below replaces it as soon as the window reports
            // its own.
            let (min, size) = logical_rect(spec.rect, spec.scale);
            let builder = egui::ViewportBuilder::default()
                .with_inner_size([size.x, size.y])
                .with_position(egui::pos2(min.x, min.y))
                .with_transparent(spec.transparent)
                .with_decorations(false)
                .with_always_on_top()
                // Overlay/chip windows must never appear in the Windows taskbar:
                // egui-winit maps `taskbar(false)` to winit's
                // `with_skip_taskbar(true)` on Windows; no-op on other OSes.
                .with_taskbar(false)
                // Windows click-through is unavailable through eframe child
                // viewports: egui-winit's `set_cursor_hittest(false)` makes winit
                // 0.30.13 add `WS_EX_TRANSPARENT | WS_EX_LAYERED`, and winit never
                // calls `SetLayeredWindowAttributes`/`UpdateLayeredWindow`, so an
                // unattributed layered window composites NOTHING (white client
                // surface + DWM shadow, GL frames never presented). A chip is
                // never interactive, so losing passthrough on Windows is harmless
                // and is what makes the chip render at all.
                .with_mouse_passthrough(spec.click_through && !cfg!(windows))
                .with_visible(false)
                .with_active(false);

            let painted_state = Arc::clone(&paint_state);
            ctx.show_viewport_deferred(viewport, builder, move |ui, _class| {
                painted_state.painted.store(true, Ordering::Relaxed);
                let alpha = f32::from_bits(painted_state.alpha.load(Ordering::Relaxed));
                let content = painted_state
                    .content
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let mut images = painted_state
                    .images
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Err(err) = paint(ui, &content, alpha, &mut images, max_texture_side) {
                    eprintln!("overlay: host overlay {id} could not paint: {err}");
                }
            });

            // Commands are dropped by eframe — and silence later requests — when
            // the viewport has no window yet.
            if !ctx.input(|input| input.raw.viewports.contains_key(&viewport)) {
                continue;
            }

            // DPI correction. Viewport commands are applied with the CHILD
            // window's pixels-per-point (`egui-winit-0.37.0/src/lib.rs:1952`,
            // `zoom_factor × window.scale_factor()`), so the builder's guess is
            // only right when the window landed on the monitor `spec.scale`
            // describes. Once the window reports its native ppp, re-send the
            // physical rect in that window's point space, and re-send whenever
            // the reported ppp changes — a chip moved to another monitor or a
            // Windows `WM_DPICHANGED` nudge then keeps its physical placement
            // instead of drifting by the DPI ratio. `None` means nothing has
            // been reported yet, so the guess stands.
            let observed_ppp = ctx.input(|input| {
                input
                    .raw
                    .viewports
                    .get(&viewport)
                    .and_then(|info| info.native_pixels_per_point)
            });
            if let Some(native) = observed_ppp {
                let ppp = ctx.zoom_factor() * native;
                if applied_ppp != Some(ppp) {
                    let (min, size) = logical_rect(spec.rect, ppp);
                    ctx.send_viewport_cmd_to(
                        viewport,
                        egui::ViewportCommand::OuterPosition(egui::pos2(min.x, min.y)),
                    );
                    ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::InnerSize(size));
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.applied_ppp = Some(ppp);
                    }
                    if self.debug.load(Ordering::Relaxed) {
                        match applied_ppp {
                            Some(previous) => eprintln!(
                                "overlay: viewport id={id} ppp {previous:.2} {ppp:.2}: re-apply OuterPosition/InnerSize"
                            ),
                            None => eprintln!(
                                "overlay: viewport id={id} ppp - {ppp:.2}: re-apply OuterPosition/InnerSize"
                            ),
                        }
                    }
                }
            }

            // Observed-vs-requested placement. `ViewportInfo::outer_rect` is the
            // rect the OS actually gave the window, in that window's POINTS;
            // multiplied by the same pixels-per-point the commands use, it is
            // comparable with the requested PHYSICAL rect. Printed only when it
            // is off by at least a pixel: a chip whose placement drifts would
            // otherwise write one line per repaint — at the steady-state cadence
            // of `REPAINT_INTERVAL` that is five per second — and a steady chip
            // with no line is the evidence placement landed (a chip that then
            // shows nothing is a presentation defect, not a placement one).
            if self.debug.load(Ordering::Relaxed) {
                let observed = ctx.input(|input| {
                    input
                        .raw
                        .viewports
                        .get(&viewport)
                        .map(|info| (info.outer_rect, info.native_pixels_per_point))
                });
                // Both fields are `Option` in egui 0.37: `None` means the
                // platform (Android/Wayland) cannot report window geometry, so
                // there is nothing to compare.
                if let Some((Some(outer_rect), Some(native))) = observed {
                    let observed = observed_rect(outer_rect, ctx.zoom_factor() * native);
                    if let Some(observed) = observed {
                        let dx = observed.x - spec.rect.x;
                        let dy = observed.y - spec.rect.y;
                        if dx.abs() >= 1.0 || dy.abs() >= 1.0 {
                            eprintln!(
                                "overlay: viewport id={id} observed_physical=({:.0},{:.0},{:.0},{:.0}) drift=({:.0},{:.0})",
                                observed.x, observed.y, observed.w, observed.h, dx, dy
                            );
                        }
                    }
                }
            }

            if !paint_state.protection_sent.load(Ordering::Relaxed) {
                paint_state.protection_sent.store(true, Ordering::Relaxed);
                ctx.send_viewport_cmd_to(
                    viewport,
                    egui::ViewportCommand::ContentProtected(spec.content_protected),
                );
            }
            // First-frame sequencing, gated on the value that actually changed.
            // `RequestPaintWhileHidden` holds for one frame
            // (`egui::ViewportCommand::RequestPaintWhileHidden`), so it is re-sent
            // while the chip is still hidden — that is what paints a hidden window.
            // Once the chip is on screen neither command is needed again, and
            // re-sending either would pin this viewport's `repaint_delay` to ZERO
            // (every `Context::send_viewport_cmd_to` calls `request_repaint_of`) and
            // deduplicate the delayed request below, defeating the throttle.
            if !paint_state.shown.load(Ordering::Relaxed) {
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::RequestPaintWhileHidden);
                if paint_state.painted.load(Ordering::Relaxed) {
                    ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Visible(true));
                    paint_state.shown.store(true, Ordering::Relaxed);
                }
            }
            // Steady-state throttle: one repaint per `REPAINT_INTERVAL` instead of
            // one per pass. Re-issued every pass because a delayed request yields
            // exactly ONE repaint; a pass woken by an immediate request (spawn, fade
            // tick, DPI correction) keeps `repaint_delay` ZERO and deduplicates this
            // request for that pass. Guarded by the window check above: a request for
            // a window-less viewport would pin it to ZERO forever.
            ctx.request_repaint_after_for(REPAINT_INTERVAL, viewport);
        }

        // The host window itself may be hidden (virshot keeps its settings window
        // hidden between hotkeys), and egui then runs no paint pass, so no child
        // viewport would ever paint or even be created. eframe skips the pass only
        // when the OS REPORTS the window as hidden — minimized or occluded
        // (`ViewportInfo::visible()`); an unknown visibility counts as visible there
        // (`visible().unwrap_or(true)`), and a visible child keeps the root pass
        // running anyway (`is_viewport_or_descendant_visible`), so asking for a
        // hidden paint in those cases would only pin ROOT's `repaint_delay` to ZERO
        // every pass and defeat the throttle.
        if has_live {
            if ctx.input(|input| {
                input
                    .raw
                    .viewports
                    .get(&egui::ViewportId::ROOT)
                    .and_then(|info| info.visible())
                    == Some(false)
            }) {
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::RequestPaintWhileHidden,
                );
            }
            ctx.request_repaint_after(REPAINT_INTERVAL);
        }

        for id in closing {
            self.close(id, ctx);
            let Ok(mut state) = self.shared.lock() else {
                continue;
            };
            state.live.remove(&id);
        }
    }

    fn close(&mut self, id: OverlayId, ctx: &egui::Context) {
        let Some(window) = self.windows.remove(&id) else {
            return;
        };
        if ctx.input(|input| input.raw.viewports.contains_key(&window.viewport)) {
            ctx.send_viewport_cmd_to(window.viewport, egui::ViewportCommand::Close);
        }
    }

    #[cfg(test)]
    fn window_count(&self) -> usize {
        self.windows.len()
    }

    #[cfg(test)]
    fn debug_enabled(&self) -> bool {
        self.debug.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn is_painted(&self, id: OverlayId) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|window| window.paint.painted.load(Ordering::Relaxed))
    }

    #[cfg(test)]
    fn alpha_of(&self, id: OverlayId) -> Option<f32> {
        self.windows
            .get(&id)
            .map(|window| f32::from_bits(window.paint.alpha.load(Ordering::Relaxed)))
    }

    #[cfg(test)]
    fn text_of(&self, id: OverlayId) -> Option<String> {
        let window = self.windows.get(&id)?;
        let content = window
            .paint
            .content
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*content {
            OverlayContent::Text(text) => Some(text.clone()),
            _ => None,
        }
    }
}

impl Default for OverlayHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for OverlayHost {
    /// Closing viewports needs the host's `Context`; if `update` never ran there
    /// is nothing to close.
    fn drop(&mut self) {
        let Some(ctx) = self.ctx.clone() else {
            return;
        };
        for window in self.windows.values() {
            ctx.send_viewport_cmd_to(window.viewport, egui::ViewportCommand::Close);
        }
        self.windows.clear();
    }
}

/// `Send + Sync` facade over the host backend's command channel.
///
/// Every method enqueue-only: none of them waits for the host thread, so a
/// capture thread can fire a toast without touching the host loop. Unlike
/// `OverlayHandle::spawn_window`, this never blocks — the viewport appears on
/// the next `OverlayHost::update`. The host must repaint for that to happen.
pub struct OverlayHostHandle {
    shared: Arc<Mutex<SharedState>>,
}

impl OverlayHostHandle {
    /// Queue a new overlay window and return its id immediately.
    ///
    /// Fails with `OverlayError::Window` when `spec.rect` has no positive,
    /// finite geometry (egui cannot create such a window), and
    /// `OverlayError::ChannelClosed` once the host backend has been dropped.
    pub fn spawn_window(
        &self,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<OverlayId, OverlayError> {
        if !geometry_is_paintable(spec.rect) {
            return Err(OverlayError::Window {
                message: format!(
                    "overlay rect {}x{} at ({}, {}) is not paintable geometry",
                    spec.rect.w, spec.rect.h, spec.rect.x, spec.rect.y
                ),
            });
        }

        let mut state = self.shared.lock().map_err(|_| OverlayError::ThreadGone)?;
        let id = OverlayId(state.next_id.fetch_add(1, Ordering::Relaxed));
        state
            .sender
            .send(OverlayCommand::Spawn {
                id,
                spec,
                content,
                alpha: alpha.clamp(0.0, 1.0),
            })
            .map_err(|_| OverlayError::ChannelClosed)?;
        state.live.insert(id);
        Ok(id)
    }

    /// Replace the content and fade alpha of a live overlay. The next
    /// `OverlayHost::update` paints the new state.
    ///
    /// Returns `OverlayError::UnknownWindow` for ids this facade did not create
    /// or already closed, and `OverlayError::ChannelClosed` once the host
    /// backend is gone.
    pub fn redraw(
        &self,
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<(), OverlayError> {
        let mut state = self.shared.lock().map_err(|_| OverlayError::ThreadGone)?;
        if !state.live.contains(&id) {
            return Err(OverlayError::UnknownWindow { id });
        }
        match state.sender.send(OverlayCommand::Redraw {
            id,
            content,
            alpha: alpha.clamp(0.0, 1.0),
        }) {
            Ok(()) => Ok(()),
            Err(_) => {
                state.live.remove(&id);
                Err(OverlayError::ChannelClosed)
            }
        }
    }

    /// Close an overlay. Closing an unknown or already-closed id is a no-op.
    pub fn hide(&self, id: OverlayId) -> Result<(), OverlayError> {
        let mut state = self.shared.lock().map_err(|_| OverlayError::ThreadGone)?;
        state.live.remove(&id);
        match state.sender.send(OverlayCommand::Hide { id }) {
            Ok(()) => Ok(()),
            Err(_) => Err(OverlayError::ChannelClosed),
        }
    }

    /// Close every overlay viewport. Idempotent; the host backend stays usable,
    /// so a later `spawn_window` opens a fresh batch.
    pub fn quit_all(&self) -> Result<(), OverlayError> {
        let mut state = self.shared.lock().map_err(|_| OverlayError::ThreadGone)?;
        state.live.clear();
        match state.sender.send(OverlayCommand::QuitAll) {
            Ok(()) => Ok(()),
            Err(_) => Err(OverlayError::ChannelClosed),
        }
    }
}

/// Stable viewport id for one overlay, matching the thread backend's naming.
fn viewport_id(id: OverlayId) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(format!("virshot-overlay-{}", id.as_u64()))
}

/// Whether egui can turn this rect into a window: positive, finite size and
/// position. `place` already rejects degenerate inputs; this guards callers
/// that build a rect by hand.
fn geometry_is_paintable(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.w > 0.0
        && rect.h > 0.0
        && rect.w.is_finite()
        && rect.h.is_finite()
}

/// Divide a PHYSICAL rect into one window's point space: origin and size in
/// points for that window's `pixels_per_point`. A non-positive or non-finite
/// ppp means nothing usable was reported, so the rect passes through unchanged
/// (`ppp = 1.0`) rather than becoming infinite.
fn logical_rect(rect: Rect, ppp: f32) -> (egui::Vec2, egui::Vec2) {
    let ppp = if ppp.is_finite() && ppp > 0.0 {
        ppp
    } else {
        1.0
    };
    (
        egui::vec2(rect.x as f32 / ppp, rect.y as f32 / ppp),
        egui::vec2(rect.w as f32 / ppp, rect.h as f32 / ppp),
    )
}

/// Turn a viewport's OBSERVED outer rect (`ViewportInfo::outer_rect`, in that
/// window's POINTS) back into PHYSICAL desktop pixels, with the same
/// pixels-per-point the placement commands use. `None` when no usable scale was
/// reported, so a window that has not reported its DPI never reports geometry.
fn observed_rect(outer: egui::Rect, ppp: f32) -> Option<Rect> {
    if !ppp.is_finite() || ppp <= 0.0 {
        return None;
    }
    let scale = f64::from(ppp);
    Some(Rect::new(
        f64::from(outer.min.x) * scale,
        f64::from(outer.min.y) * scale,
        f64::from(outer.width()) * scale,
        f64::from(outer.height()) * scale,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn assert_send_sync<T: Send + Sync>() {}

    /// The toast chip shape from `virshot`'s tests, at an explicit origin.
    /// Physical pixels, on a monitor whose DPI scale is unknown (`1.0`).
    fn spec_at(x: f64, y: f64) -> OverlayWindowSpec {
        spec_on(Rect::new(x, y, 200.0, 36.0), 1.0)
    }

    /// A chip with an explicit physical rect and target-monitor scale.
    fn spec_on(rect: Rect, scale: f32) -> OverlayWindowSpec {
        OverlayWindowSpec {
            rect,
            scale,
            transparent: true,
            click_through: true,
            content_protected: true,
        }
    }

    fn chip(text: &str) -> OverlayContent {
        OverlayContent::Text(text.to_owned())
    }

    /// A chip image: the host thread decodes and uploads it, so the bytes must
    /// be a real encoded image.
    fn png_chip() -> OverlayContent {
        let image = image::DynamicImage::ImageRgba8(image::ImageBuffer::from_pixel(
            4,
            4,
            image::Rgba([0x26, 0x26, 0x2A, 0xFF]),
        ));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("a tiny png encodes");
        OverlayContent::Image(bytes)
    }

    /// A context that supports multiple viewports. `Context::default()` embeds
    /// them instead, which paints the overlay inside the host window; eframe on
    /// desktop disables embedding (`eframe/src/native/winit_integration.rs:59`).
    fn host_context() -> egui::Context {
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        ctx
    }

    /// A viewport that already has a real window, reporting the DPI scale winit
    /// gave it (`ViewportInfo::native_pixels_per_point`, before egui's zoom).
    fn window_with_scale(scale: f32) -> egui::ViewportInfo {
        egui::ViewportInfo {
            native_pixels_per_point: Some(scale),
            ..Default::default()
        }
    }

    #[test]
    fn overlay_host_handle_is_send_and_sync() {
        assert_send_sync::<OverlayHostHandle>();
        assert_send_sync::<OverlayError>();
    }

    #[test]
    fn spawn_window_enqueues_without_waiting_for_a_host_update() {
        // No `update` ever runs, so a blocking handshake would hang this join.
        let host = OverlayHost::new();
        let spawning = host.handle();
        let spawned = std::thread::spawn(move || {
            spawning.spawn_window(spec_at(10.0, 20.0), chip("chip"), 1.0)
        });
        let id = spawned
            .join()
            .expect("the spawning thread returned")
            .expect("enqueueing a spawn succeeds");
        // The viewport is created inside `update`, on the host thread.
        assert_eq!(host.window_count(), 0);
        assert!(host.handle().redraw(id, chip("chip"), 0.5).is_ok());
    }

    #[test]
    fn update_turns_a_queued_spawn_into_a_window() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        host.update(&host_context());
        assert_eq!(host.window_count(), 1);
        // First-frame sequencing: a fresh window is hidden until it has painted.
        assert!(!host.is_painted(id));
        assert_eq!(host.alpha_of(id), Some(1.0));
    }

    #[test]
    fn update_applies_the_latest_redraw_for_one_window() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("first-best"), 1.0)
            .expect("enqueueing a spawn succeeds");
        handle
            .redraw(id, chip("older"), 0.5)
            .expect("the window is live");
        handle
            .redraw(id, chip("latest"), 0.25)
            .expect("the window is live");
        host.update(&host_context());
        assert_eq!(host.alpha_of(id), Some(0.25));
        assert_eq!(host.text_of(id).as_deref(), Some("latest"));
    }

    #[test]
    fn update_applies_a_queued_hide_and_drops_the_window() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        host.update(&host_context());
        assert_eq!(host.window_count(), 1);

        handle.hide(id).expect("hiding a live window succeeds");
        host.update(&host_context());
        assert_eq!(host.window_count(), 0);
    }

    #[test]
    fn quit_all_clears_the_live_windows() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let first = handle
            .spawn_window(spec_at(0.0, 0.0), chip("first"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let second = handle
            .spawn_window(spec_at(400.0, 0.0), chip("second"), 1.0)
            .expect("enqueueing a spawn succeeds");
        host.update(&host_context());
        assert_eq!(host.window_count(), 2);

        handle.quit_all().expect("quitting succeeds");
        assert!(matches!(
            handle.redraw(first, chip("gone"), 1.0),
            Err(OverlayError::UnknownWindow { id } ) if id == first
        ));
        assert!(matches!(
            handle.redraw(second, chip("gone"), 1.0),
            Err(OverlayError::UnknownWindow { id } ) if id == second
        ));
        host.update(&host_context());
        assert_eq!(host.window_count(), 0);
    }

    #[test]
    fn quit_all_before_any_window_exists_is_a_no_op() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        assert!(handle.quit_all().is_ok());
        host.update(&host_context());
        assert_eq!(host.window_count(), 0);
    }

    #[test]
    fn redraw_on_a_never_created_window_reports_unknown_window() {
        let host = OverlayHost::new();
        let handle = host.handle();
        let result = handle.redraw(OverlayId(7), chip("chip"), 1.0);
        assert!(matches!(
            result,
            Err(OverlayError::UnknownWindow { id: OverlayId(7) })
        ));
    }

    #[test]
    fn redraw_after_hide_reports_unknown_window() {
        let host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        handle.hide(id).expect("hiding a live window succeeds");
        assert!(matches!(
            handle.redraw(id, chip("chip"), 1.0),
            Err(OverlayError::UnknownWindow { id: closed } ) if closed == id
        ));
    }

    #[test]
    fn hide_on_a_never_created_window_is_a_no_op() {
        let host = OverlayHost::new();
        let handle = host.handle();
        assert!(handle.hide(OverlayId(7)).is_ok());
    }

    #[test]
    fn spawn_window_assigns_distinct_ids() {
        let host = OverlayHost::new();
        let handle = host.handle();
        let first = handle
            .spawn_window(spec_at(0.0, 0.0), chip("first"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let second = handle
            .spawn_window(spec_at(0.0, 0.0), chip("second"), 1.0)
            .expect("enqueueing a spawn succeeds");
        assert_ne!(first, second);
    }

    #[test]
    fn set_monitors_roundtrips_through_monitors() {
        let host = OverlayHost::new();
        assert!(host.monitors().is_empty());
        let monitors = vec![
            MonitorRect::new(Rect::new(0.0, 0.0, 1920.0, 1080.0), 1.0),
            MonitorRect::new(Rect::new(1920.0, 0.0, 1280.0, 720.0), 1.5),
        ];
        host.set_monitors(monitors.clone());
        assert_eq!(host.monitors(), monitors);
        host.set_monitors(vec![MonitorRect::new(
            Rect::new(0.0, 0.0, 1280.0, 720.0),
            1.0,
        )]);
        assert_eq!(host.monitors().len(), 1);
    }

    #[test]
    fn spawn_window_rejects_a_degenerate_rect() {
        let host = OverlayHost::new();
        let handle = host.handle();
        for rect in [
            Rect::new(0.0, 0.0, 0.0, 36.0),
            Rect::new(0.0, 0.0, 200.0, -1.0),
            Rect::new(f64::NAN, 0.0, 200.0, 36.0),
        ] {
            let degenerate = OverlayWindowSpec {
                rect,
                scale: 1.0,
                transparent: true,
                click_through: false,
                content_protected: true,
            };
            assert!(
                matches!(
                    handle.spawn_window(degenerate, chip("chip"), 1.0),
                    Err(OverlayError::Window { .. })
                ),
                "a degenerate rect must not become a viewport"
            );
        }
        assert!(handle.quit_all().is_ok());
    }

    #[test]
    fn update_registers_the_viewport_on_the_pass_that_creates_the_window() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let ctx = host_context();
        host.update(&ctx);

        let output = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let viewport = output
            .viewport_output
            .get(&viewport_id(id))
            .expect("the overlay viewport is registered");
        let builder = viewport.builder.clone();
        let commands = viewport.commands.clone();
        let has_callback = viewport.viewport_ui_cb.is_some();
        output.drop_without_applying_deltas();

        assert!(
            has_callback,
            "a deferred viewport carries its paint callback"
        );
        assert_eq!(builder.inner_size, Some(egui::vec2(200.0, 36.0)));
        assert_eq!(builder.position, Some(egui::pos2(0.0, 0.0)));
        assert_eq!(builder.transparent, Some(true));
        assert_eq!(builder.decorations, Some(false));
        // The builder rule is `spec.click_through && !cfg!(windows)`
        // (host.rs:346-354): egui-winit's `set_cursor_hittest(false)` makes
        // winit add `WS_EX_TRANSPARENT | WS_EX_LAYERED` without ever
        // attributing the layered window, so passthrough is dropped on
        // Windows to keep the chip compositing at all. `spec_at` sets
        // `click_through: true`, so the builder value is `!cfg!(windows)`.
        assert_eq!(
            builder.mouse_passthrough,
            Some(!cfg!(windows)),
            "passthrough must match the builder rule at host.rs:346-354"
        );
        assert_eq!(builder.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(
            builder.visible,
            Some(false),
            "created hidden: an unpainted window is the white strip"
        );
        assert_eq!(builder.active, Some(false), "never steals focus");
        assert!(
            commands.is_empty(),
            "eframe drops commands for a viewport with no window yet"
        );
    }

    #[test]
    fn a_window_is_painted_while_hidden_before_it_is_shown() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();

        host.update(&ctx);
        let output = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let callback = output.viewport_output[&viewport]
            .viewport_ui_cb
            .clone()
            .expect("the deferred viewport carries its paint callback");
        output.drop_without_applying_deltas();

        // The backend now has a real window; egui runs the callback while the
        // window is still hidden, so the first frame lands before it is shown.
        let mut input = egui::RawInput::default();
        input
            .viewports
            .insert(viewport, egui::ViewportInfo::default());
        ctx.run_ui(input.clone(), |ui| callback(ui))
            .drop_without_applying_deltas();
        assert!(
            host.is_painted(id),
            "the first frame is painted while hidden"
        );

        host.update(&ctx);
        let output = ctx.run_ui(input, |_ui| {});
        let commands = output.viewport_output[&viewport].commands.clone();
        output.drop_without_applying_deltas();

        assert!(
            commands.contains(&egui::ViewportCommand::ContentProtected(true)),
            "content protection is sent once the window exists"
        );
        assert!(
            commands.contains(&egui::ViewportCommand::RequestPaintWhileHidden),
            "holds one frame, so it is re-sent every pass"
        );
        assert!(
            commands.contains(&egui::ViewportCommand::Visible(true)),
            "shown only after it has pixels"
        );
    }

    /// Raw input in which the OS REPORTS the root window as hidden (minimized).
    ///
    /// `ViewportInfo::visible()` (`egui-0.37.0/src/data/input/viewport_info.rs:98`)
    /// is the only hidden signal eframe has, and it treats an unknown visibility as
    /// visible (`visible().unwrap_or(true)`), so a plain `RawInput::default()`
    /// (all flags `None`) means "the host window paints anyway".
    fn input_with_hidden_root() -> egui::RawInput {
        let mut input = egui::RawInput::default();
        input.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                minimized: Some(true),
                ..Default::default()
            },
        );
        input
    }

    #[test]
    fn an_alive_overlay_asks_the_root_viewport_to_paint_while_hidden() {
        // A hidden host window (virshot keeps its settings window hidden) runs
        // no paint pass, so egui would never paint or even create the child
        // window — the host asks the root for a hidden paint for a live overlay.
        // The request is gated on the root being KNOWN hidden: sending it every
        // pass would pin ROOT's `repaint_delay` to ZERO and defeat the throttle.
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let ctx = host_context();
        host.update(&ctx);

        let output = ctx.run_ui(input_with_hidden_root(), |_ui| host.paint(&ctx));
        let root_commands = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the root viewport is always reported")
            .commands
            .clone();
        output.drop_without_applying_deltas();

        assert!(
            root_commands.contains(&egui::ViewportCommand::RequestPaintWhileHidden),
            "a live overlay asks a KNOWN-hidden root to paint while hidden"
        );
        assert!(!host.is_painted(id), "nothing has painted yet");

        // A root whose visibility is unknown (or reported visible) gets no hidden
        // paint request: eframe runs its pass regardless, so asking would only
        // cost a repaint every pass.
        let output = ctx.run_ui(egui::RawInput::default(), |_ui| host.paint(&ctx));
        let root_commands = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the root viewport is always reported")
            .commands
            .clone();
        output.drop_without_applying_deltas();
        assert!(
            !root_commands.contains(&egui::ViewportCommand::RequestPaintWhileHidden),
            "no hidden-paint request while the root is not known hidden"
        );

        // With no overlay alive the host gets no extra paint requests at all.
        handle.hide(id).expect("hiding a live window succeeds");
        host.update(&ctx);
        let output = ctx.run_ui(input_with_hidden_root(), |_ui| {});
        let root_commands = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the root viewport is always reported")
            .commands
            .clone();
        output.drop_without_applying_deltas();
        assert!(
            !root_commands.contains(&egui::ViewportCommand::RequestPaintWhileHidden),
            "no overlay, no hidden-paint request on the host loop"
        );
    }

    #[test]
    fn image_content_is_decoded_and_uploaded_during_the_child_paint() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), png_chip(), 1.0)
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();

        host.update(&ctx);
        let output = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let callback = output.viewport_output[&viewport]
            .viewport_ui_cb
            .clone()
            .expect("the deferred viewport carries its paint callback");
        output.drop_without_applying_deltas();

        let before = ctx.tex_manager().read().allocated().len();
        ctx.run_ui(egui::RawInput::default(), |ui| callback(ui))
            .drop_without_applying_deltas();

        assert!(host.is_painted(id));
        assert!(
            ctx.tex_manager().read().allocated().len() > before,
            "the image was decoded and uploaded on the host thread"
        );
    }

    // ---------- physical → point conversion ----------

    #[test]
    fn logical_rect_converts_physical_pixels_into_the_window_point_space() {
        // A 1920 px desktop origin is 1280 points on a 150% DPI window.
        let (min, size) = logical_rect(Rect::new(1920.0, 0.0, 1920.0, 1080.0), 1.5);
        assert_eq!(min, egui::vec2(1280.0, 0.0));
        assert_eq!(size, egui::vec2(1280.0, 720.0));

        // At 100% DPI the physical rect and the point rect are the same numbers.
        let (min, size) = logical_rect(Rect::new(1920.0, 0.0, 1920.0, 1080.0), 1.0);
        assert_eq!(min, egui::vec2(1920.0, 0.0));
        assert_eq!(size, egui::vec2(1920.0, 1080.0));
    }

    #[test]
    fn logical_rect_passes_the_rect_through_when_no_usable_scale_is_given() {
        for ppp in [0.0, -1.0, f32::NAN] {
            let (min, size) = logical_rect(Rect::new(100.0, 200.0, 200.0, 36.0), ppp);
            assert_eq!(min, egui::vec2(100.0, 200.0), "ppp {ppp} must not scale");
            assert_eq!(size, egui::vec2(200.0, 36.0), "ppp {ppp} must not scale");
        }
    }

    // ---------- debug diagnostics ----------

    #[test]
    fn observed_rect_scales_the_reported_outer_rect_back_to_physical_pixels() {
        // The OS gave a 200% DPI window 960x0 points at 640x360 points: the
        // debug line compares 1920x0 px at 1280x720 px with the requested rect.
        let outer = egui::Rect::from_min_size(egui::pos2(960.0, 0.0), egui::vec2(640.0, 360.0));
        assert_eq!(
            observed_rect(outer, 2.0),
            Some(Rect::new(1920.0, 0.0, 1280.0, 720.0)),
            "points are multiplied by the window's own pixels-per-point"
        );
    }

    #[test]
    fn observed_rect_needs_a_usable_scale() {
        let outer = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 36.0));
        for ppp in [0.0, -1.0, f32::NAN] {
            assert_eq!(
                observed_rect(outer, ppp),
                None,
                "ppp {ppp} must report nothing"
            );
        }
    }

    #[test]
    fn set_debug_starts_off_and_roundtrips() {
        let host = OverlayHost::new();
        assert!(!host.debug_enabled(), "debug lines are opt-in");
        host.set_debug(true);
        assert!(host.debug_enabled());
        host.set_debug(false);
        assert!(!host.debug_enabled());
    }

    #[test]
    fn builder_positions_the_chip_using_the_target_monitor_scale_guess() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(
                spec_on(Rect::new(1920.0, 0.0, 1280.0, 720.0), 1.5),
                chip("chip"),
                1.0,
            )
            .expect("enqueueing a spawn succeeds");
        let ctx = host_context();
        host.update(&ctx);

        let output = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let builder = output.viewport_output[&viewport_id(id)].builder.clone();
        output.drop_without_applying_deltas();

        assert_eq!(builder.position, Some(egui::pos2(1280.0, 0.0)));
        assert_eq!(
            builder.inner_size,
            Some(egui::vec2(1280.0 / 1.5, 720.0 / 1.5)),
            "the builder divides the physical rect by the target monitor scale"
        );
    }

    #[test]
    fn update_corrects_placement_to_the_scale_the_window_reports() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        // A chip on a secondary monitor: builder guessed 100% DPI, the window
        // that winit created reports 200%.
        let id = handle
            .spawn_window(
                spec_on(Rect::new(1920.0, 0.0, 1280.0, 720.0), 1.0),
                chip("chip"),
                1.0,
            )
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();
        host.update(&ctx);

        let mut input = egui::RawInput::default();
        input.viewports.insert(viewport, window_with_scale(2.0));
        ctx.run_ui(input.clone(), |_ui| {})
            .drop_without_applying_deltas();

        host.update(&ctx);
        let output = ctx.run_ui(input.clone(), |_ui| {});
        let commands = output.viewport_output[&viewport].commands.clone();
        output.drop_without_applying_deltas();

        assert!(
            commands.contains(&egui::ViewportCommand::OuterPosition(egui::pos2(
                960.0, 0.0
            ))),
            "position re-expressed in the window's own points"
        );
        assert!(
            commands.contains(&egui::ViewportCommand::InnerSize(egui::vec2(640.0, 360.0))),
            "size re-expressed in the window's own points"
        );
    }

    #[test]
    fn update_re_sends_placement_only_when_the_reported_scale_changes() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let rect = Rect::new(1920.0, 0.0, 1280.0, 720.0);
        let id = handle
            .spawn_window(spec_on(rect, 1.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();
        host.update(&ctx);

        // First correction: the window lands at 200%.
        let mut at_two = egui::RawInput::default();
        at_two.viewports.insert(viewport, window_with_scale(2.0));
        ctx.run_ui(at_two.clone(), |_ui| {})
            .drop_without_applying_deltas();
        host.update(&ctx);
        ctx.run_ui(at_two.clone(), |_ui| {})
            .drop_without_applying_deltas();

        // A Windows `WM_DPICHANGED` nudge moves the chip to a 150% monitor.
        let mut at_one_five = egui::RawInput::default();
        at_one_five
            .viewports
            .insert(viewport, window_with_scale(1.5));
        ctx.run_ui(at_one_five.clone(), |_ui| {})
            .drop_without_applying_deltas();
        host.update(&ctx);
        let output = ctx.run_ui(at_one_five.clone(), |_ui| {});
        let commands = output.viewport_output[&viewport].commands.clone();
        output.drop_without_applying_deltas();

        assert!(
            commands.contains(&egui::ViewportCommand::OuterPosition(egui::pos2(
                1280.0, 0.0
            ))),
            "the new scale gets a fresh position"
        );
        assert!(
            commands.contains(&egui::ViewportCommand::InnerSize(egui::vec2(
                1280.0 / 1.5,
                720.0 / 1.5
            ))),
            "the new scale gets a fresh size"
        );

        // Same scale again: nothing is re-sent, so the WM sees no churn.
        host.update(&ctx);
        let output = ctx.run_ui(at_one_five, |_ui| {});
        let commands = output.viewport_output[&viewport].commands.clone();
        output.drop_without_applying_deltas();
        assert!(
            !commands.iter().any(|cmd| matches!(
                cmd,
                egui::ViewportCommand::OuterPosition(_) | egui::ViewportCommand::InnerSize(_)
            )),
            "an unchanged scale sends no placement commands"
        );
    }

    #[test]
    fn update_defers_the_correction_until_the_window_reports_a_scale() {
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(
                spec_on(Rect::new(1920.0, 0.0, 1280.0, 720.0), 1.0),
                chip("chip"),
                1.0,
            )
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();
        host.update(&ctx);

        // A window exists but has not reported its DPI yet.
        let mut input = egui::RawInput::default();
        input
            .viewports
            .insert(viewport, egui::ViewportInfo::default());
        ctx.run_ui(input.clone(), |_ui| {})
            .drop_without_applying_deltas();

        host.update(&ctx);
        let output = ctx.run_ui(input, |_ui| {});
        let commands = output.viewport_output[&viewport].commands.clone();
        output.drop_without_applying_deltas();

        assert!(
            commands.contains(&egui::ViewportCommand::RequestPaintWhileHidden),
            "the paint pass still runs"
        );
        assert!(
            !commands.iter().any(|cmd| matches!(
                cmd,
                egui::ViewportCommand::OuterPosition(_) | egui::ViewportCommand::InnerSize(_)
            )),
            "no correction while the window reports no pixels-per-point"
        );
    }

    // ---------- repaint scheduling ----------

    /// The interval `REPAINT_INTERVAL` implements, pinned here (not read from the
    /// const) so the throttle cannot drift from the value the tests were written
    /// against.
    const REPAINT_INTERVAL_MS: u64 = 200;

    /// Every repaint request a context saw, in request order.
    ///
    /// Used by the `drain` tests, which run outside a paint pass: egui overwrites a
    /// request made outside a pass at `begin_pass_repaint_logic`
    /// (`egui-0.37.0/src/context.rs:100-126`), so
    /// `FullOutput::viewport_output[id].repaint_delay` cannot show what `drain`
    /// asked for. `Context::set_repaint_observer` fires for EVERY request including
    /// deduped ones (`context.rs:160-162`), carrying
    /// `RequestRepaintInfo { viewport_id, delay, .. }` — the only public surface
    /// that tells an immediate request from a delayed one without a pass.
    ///
    /// A pass-based test asserts the EFFECTIVE schedule through `repaint_delay`
    /// instead (`paint_steady_live_chip_schedules_delayed_repaint`).
    ///
    /// The observer runs while the `Context` is locked, so it must not call back
    /// into it: this one only appends to the log behind its own mutex.
    #[derive(Default)]
    struct RepaintLog {
        entries: Arc<Mutex<Vec<(egui::ViewportId, Duration)>>>,
    }

    impl RepaintLog {
        /// Install as the context's repaint observer. Only one observer can be
        /// set — every `set_repaint_observer` call overrides the previous one.
        fn install(ctx: &egui::Context) -> Self {
            let log = Self::default();
            let observed = Arc::clone(&log.entries);
            ctx.set_repaint_observer(move |info| {
                if let Ok(mut entries) = observed.lock() {
                    entries.push((info.viewport_id, info.delay));
                }
            });
            log
        }

        /// Forget everything recorded so far, so a test observes only the pass
        /// under test and not the setup passes before it.
        fn clear(&self) {
            if let Ok(mut entries) = self.entries.lock() {
                entries.clear();
            }
        }

        /// Delays requested for `viewport`, in request order.
        fn delays_for(&self, viewport: egui::ViewportId) -> Vec<Duration> {
            let Ok(entries) = self.entries.lock() else {
                return Vec::new();
            };
            entries
                .iter()
                .filter(|(id, _)| *id == viewport)
                .map(|(_, delay)| *delay)
                .collect()
        }

        /// Whether any request asked for exactly `delay` on `viewport`.
        fn requested(&self, viewport: egui::ViewportId, delay: Duration) -> bool {
            self.delays_for(viewport)
                .into_iter()
                .any(|observed| observed == delay)
        }
    }

    /// Raw input reporting a real chip window with the given DPI scale.
    fn input_with_chip_window(viewport: egui::ViewportId, scale: f32) -> egui::RawInput {
        let mut input = egui::RawInput::default();
        input.viewports.insert(viewport, window_with_scale(scale));
        input
    }

    /// The chip's paint callback, as egui hands it to the backend
    /// (`egui::DeferredViewportUiCallback`).
    type ChipCallback = Arc<dyn Fn(&mut egui::Ui) + Send + Sync>;

    /// One host-loop pass: `host.paint` runs INSIDE the pass, because egui overwrites
    /// a repaint request made outside a pass at `begin_pass_repaint_logic`
    /// (`context.rs:100-126`); only an in-pass request becomes the EFFECTIVE schedule
    /// the pass reports in `FullOutput::viewport_output[id].repaint_delay`.
    fn host_pass(
        ctx: &egui::Context,
        host: &mut OverlayHost,
        input: &egui::RawInput,
    ) -> egui::FullOutput {
        ctx.run_ui(input.clone(), |_ui| host.paint(ctx))
    }

    /// One chip pass, exactly how eframe runs a Deferred viewport
    /// (`eframe-0.37.0/src/native/glow_integration.rs:1781`: `run_ui` with
    /// `RawInput::viewport_id` set to the child and its stored callback as the root
    /// ui). That pass resets the chip's schedule: with no outstanding request,
    /// `begin_pass_repaint_logic` puts `repaint_delay` back to `Duration::MAX`
    /// (`context.rs:113-115`), which is what lets the NEXT host pass schedule a
    /// throttled repaint instead of deduplicating against a leftover ZERO.
    fn chip_pass(
        ctx: &egui::Context,
        viewport: egui::ViewportId,
        input: &egui::RawInput,
        callback: &ChipCallback,
    ) -> egui::FullOutput {
        let mut input = input.clone();
        input.viewport_id = viewport;
        let callback = Arc::clone(callback);
        ctx.run_ui(input, move |ui| callback(ui))
    }

    /// The repaint a pass is scheduled for, per viewport.
    fn scheduled_repaint(output: &egui::FullOutput, viewport: egui::ViewportId) -> Duration {
        output.viewport_output[&viewport].repaint_delay
    }

    /// Drive a chip through the pass order eframe uses — host pass, chip pass, host
    /// pass, … — up to the repaint the `Visible(true)` request bought, and return the
    /// raw input that reports its window plus the viewport's paint callback.
    fn drive_to_shown(
        host: &mut OverlayHost,
        ctx: &egui::Context,
        viewport: egui::ViewportId,
    ) -> (egui::RawInput, ChipCallback) {
        // Register the deferred viewport: it exists but has no window yet.
        let output = ctx.run_ui(egui::RawInput::default(), |_ui| host.update(ctx));
        let callback = output.viewport_output[&viewport]
            .viewport_ui_cb
            .clone()
            .expect("the deferred viewport carries its paint callback");
        output.drop_without_applying_deltas();

        let input = input_with_chip_window(viewport, 1.0);

        // Host pass: the OS created the window and reported its DPI, so `paint` takes
        // the has-window branch — content protection, hidden paint, DPI correction.
        host_pass(ctx, host, &input).drop_without_applying_deltas();

        // Chip pass: the first frame is painted while the window is still hidden.
        chip_pass(ctx, viewport, &input, &callback).drop_without_applying_deltas();

        // Host pass: the chip has pixels, so it is shown (`Visible(true)`).
        host_pass(ctx, host, &input).drop_without_applying_deltas();

        // Chip pass: the second repaint an immediate (ZERO-delay) request bought
        // (`context.rs:141-145`); after it the chip is idle.
        chip_pass(ctx, viewport, &input, &callback).drop_without_applying_deltas();

        (input, callback)
    }

    #[test]
    fn paint_steady_live_chip_schedules_delayed_repaint() {
        // A chip that already has a window changes only when a command arrives, so a
        // steady host pass must schedule the next repaint for the throttle interval
        // instead of demanding one right now — on the chip's own viewport (a Deferred
        // child is NOT woken by a root repaint) and on the host loop. A second cycle
        // keeps the schedule, because a delayed request buys exactly ONE repaint
        // (`context.rs:147-151`) and `paint` must re-issue it to stay alive.
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();
        let (input, callback) = drive_to_shown(&mut host, &ctx, viewport);

        let interval = Duration::from_millis(REPAINT_INTERVAL_MS);
        for cycle in 1..=2 {
            chip_pass(&ctx, viewport, &input, &callback).drop_without_applying_deltas();
            let output = host_pass(&ctx, &mut host, &input);
            assert_eq!(
                scheduled_repaint(&output, viewport),
                interval,
                "cycle {cycle}: a live chip must idle for {interval:?}, not repaint now"
            );
            assert_eq!(
                scheduled_repaint(&output, egui::ViewportId::ROOT),
                interval,
                "cycle {cycle}: the host loop must idle for {interval:?} too"
            );
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn paint_sends_viewport_commands_only_when_a_value_changes() {
        // The throttle depends on the change gate: `Context::send_viewport_cmd_to`
        // calls `request_repaint_of` (`context.rs:4569-4573`), so re-sending an
        // unchanged command would pin the chip's `repaint_delay` to ZERO and
        // deduplicate the delayed request of the same pass. Commands whose value has
        // already reached the window (protection, hidden paint, show) and the
        // placement correction for the reported scale must therefore be silent.
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();
        let (input, callback) = drive_to_shown(&mut host, &ctx, viewport);

        chip_pass(&ctx, viewport, &input, &callback).drop_without_applying_deltas();
        let output = host_pass(&ctx, &mut host, &input);
        let commands = output.viewport_output[&viewport].commands.clone();
        let delay = scheduled_repaint(&output, viewport);
        output.drop_without_applying_deltas();

        assert!(
            !commands.iter().any(|cmd| matches!(
                cmd,
                egui::ViewportCommand::OuterPosition(_)
                    | egui::ViewportCommand::InnerSize(_)
                    | egui::ViewportCommand::Visible(_)
                    | egui::ViewportCommand::ContentProtected(_)
                    | egui::ViewportCommand::RequestPaintWhileHidden
            )),
            "an unchanged chip must send no viewport command, got {commands:?}"
        );
        assert_eq!(
            delay,
            Duration::from_millis(REPAINT_INTERVAL_MS),
            "with no command the chip keeps the throttled schedule, got {delay:?}"
        );

        // A reported scale change is a value change: the correction is sent and that
        // pass repaints immediately — placement must never wait for the interval.
        let output = host_pass(&ctx, &mut host, &input_with_chip_window(viewport, 1.5));
        let commands = output.viewport_output[&viewport].commands.clone();
        let delay = scheduled_repaint(&output, viewport);
        output.drop_without_applying_deltas();

        assert!(
            commands
                .iter()
                .any(|cmd| matches!(cmd, egui::ViewportCommand::OuterPosition(_)))
                && commands
                    .iter()
                    .any(|cmd| matches!(cmd, egui::ViewportCommand::InnerSize(_))),
            "the corrected placement is re-sent when the window reports a new scale, got {commands:?}"
        );
        assert_eq!(
            delay,
            Duration::ZERO,
            "a placement correction repaints the chip now, got {delay:?}"
        );
    }

    #[test]
    fn drain_spawn_still_requests_immediate_repaint() {
        // A spawn must be registered on the very next pass, so the host loop gets
        // an immediate (ZERO-delay) request. The throttle keeps this invariant: a
        // delayed request would leave the child window uncreated (and a
        // window-less viewport must not be pinned via `request_repaint_of` — see the
        // hazard note in the `Spawn` branch of `drain`).
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let ctx = host_context();
        let log = RepaintLog::install(&ctx);

        handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        host.drain(&ctx);

        assert!(
            log.requested(egui::ViewportId::ROOT, Duration::ZERO),
            "a spawn must wake the host loop immediately, got {:?}",
            log.delays_for(egui::ViewportId::ROOT)
        );
    }

    #[test]
    fn drain_redraw_requests_immediate_repaint_for_the_chip_viewport() {
        // A Deferred viewport repaints independently of its parent, so a root
        // `request_repaint()` never wakes the chip: new content would sit
        // unpainted until the chip's own next pass. A Redraw of a chip that HAS a
        // window must therefore wake that viewport immediately — this is what keeps
        // a fade tick animating at 25 ms despite the steady-state throttle.
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let viewport = viewport_id(id);
        let ctx = host_context();
        let log = RepaintLog::install(&ctx);

        host.update(&ctx);
        let mut input = egui::RawInput::default();
        input.viewports.insert(viewport, window_with_scale(1.0));
        ctx.run_ui(input, |_ui| {}).drop_without_applying_deltas();

        handle
            .redraw(id, chip("updated"), 1.0)
            .expect("the window is live");
        log.clear();
        host.drain(&ctx);

        assert!(
            log.requested(viewport, Duration::ZERO),
            "a Redraw must wake the chip's own viewport, got {:?}",
            log.delays_for(viewport)
        );
        assert!(
            log.requested(egui::ViewportId::ROOT, Duration::ZERO),
            "the host loop must wake too, so the viewport stays registered, got {:?}",
            log.delays_for(egui::ViewportId::ROOT)
        );
    }
}
