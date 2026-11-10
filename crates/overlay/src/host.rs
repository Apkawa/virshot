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

use crate::content::{ImageCache, OverlayContent, paint};
use crate::error::OverlayError;
use crate::placement::{MonitorRect, Rect};
use crate::{OverlayId, OverlayWindowSpec};

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
                    // redraw (`gui.rs:108-114`). The host loop repaint is enough —
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
                    // The per-viewport repaint request is issued by `paint`, which
                    // gates it on the viewport having a real window.
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
    /// sequencing ported from `virshot/src/gui.rs`: the window is created
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

            // egui needs `show_viewport_deferred` on EVERY pass the child exists
            // (`gui.rs:1250`): the pass that registers it is the pass that
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
            // the viewport has no window yet (`gui.rs:1284`).
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
            // is off by at least a pixel: a steady chip would otherwise write
            // one line every repaint, and a steady chip with no line is the
            // evidence placement landed (a chip that then shows nothing is a
            // presentation defect, not a placement one).
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
            // Holds for one frame (`egui::ViewportCommand::RequestPaintWhileHidden`),
            // so it is re-sent every pass: this is what paints a hidden window.
            ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::RequestPaintWhileHidden);
            if paint_state.painted.load(Ordering::Relaxed) {
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Visible(true));
            }
            ctx.request_repaint_of(viewport);
        }

        // The host window itself may be hidden (virshot keeps its settings window
        // hidden between hotkeys); egui runs no pass then, so no child viewport
        // would ever paint or even be created. `gui.rs:1292-1296` asks the root
        // for a hidden paint whenever an overlay is alive.
        if has_live {
            ctx.request_repaint();
            ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::RequestPaintWhileHidden,
            );
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
            "created hidden: an unpainted window is the white strip (gui.rs:1195-1205)"
        );
        assert_eq!(builder.active, Some(false), "never steals focus");
        assert!(
            commands.is_empty(),
            "eframe drops commands for a viewport with no window yet (gui.rs:1284)"
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

    #[test]
    fn an_alive_overlay_asks_the_root_viewport_to_paint_while_hidden() {
        // A hidden host window (virshot keeps its settings window hidden) runs
        // no pass, so egui would never paint or even create the child window
        // (`gui.rs:1292-1296` does the same for a live toast).
        let mut host = OverlayHost::new();
        let handle = host.handle();
        let id = handle
            .spawn_window(spec_at(0.0, 0.0), chip("chip"), 1.0)
            .expect("enqueueing a spawn succeeds");
        let ctx = host_context();
        host.update(&ctx);

        let output = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let root_commands = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("the root viewport is always reported")
            .commands
            .clone();
        output.drop_without_applying_deltas();

        assert!(
            root_commands.contains(&egui::ViewportCommand::RequestPaintWhileHidden),
            "a live overlay asks the root to paint while it is hidden"
        );
        assert!(!host.is_painted(id), "nothing has painted yet");

        // With no overlay alive the host gets no extra paint requests at all.
        handle.hide(id).expect("hiding a live window succeeds");
        host.update(&ctx);
        let output = ctx.run_ui(egui::RawInput::default(), |_ui| {});
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
}
