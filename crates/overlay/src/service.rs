//! The overlay event-loop thread.
//!
//! winit 0.30 allows exactly one `EventLoop` per process (`EventLoopBuilder::build`
//! returns `EventLoopError::RecreationAttempt` for a second one), so all overlay
//! windows live on a single thread inside a single event loop instead of one
//! thread per window. Commands arrive as `LoopEvent`s through the facade's
//! `EventLoopProxy`, which also wakes the loop out of `ControlFlow::Wait`.
//!
//! Painting mirrors `eframe-0.37.0/src/native/glow_integration.rs`: egui input →
//! `Context::run_ui` → texture deltas → tessellation → `Painter::paint_primitives`
//! → texture frees → `swap_buffers`. `FullOutput::viewport_output` is ignored —
//! overlay content never spawns viewports, and this crate has no timers, so
//! egui's repaint delays are not honored: callers drive repaints with `redraw`.

use std::collections::HashMap;
use std::sync::mpsc::Sender;

use egui::ViewportId;
use egui_glow::Painter;
use egui_winit::State;
use glutin::surface::{Surface, WindowSurface};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

use crate::content::{ImageCache, OverlayContent, paint};
use crate::error::OverlayError;
use crate::gl_boot::{Backend, WindowSurfacePair};
use crate::{OverlayId, OverlayWindowSpec};

/// A command for the overlay thread, delivered as a winit user event.
pub(crate) enum LoopEvent {
    /// Create an overlay window and report readiness on `ack` before returning.
    Create {
        id: OverlayId,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
        ack: Sender<Result<(), OverlayError>>,
    },
    /// Replace the content and fade alpha of an existing window and repaint it.
    Redraw {
        id: OverlayId,
        content: OverlayContent,
        alpha: f32,
    },
    /// Close an existing window. Unknown ids are ignored.
    Hide { id: OverlayId },
    /// Close every window and exit the event loop.
    QuitAll,
}

/// One overlay window plus everything needed to paint it.
struct WindowEntry {
    window: Window,
    surface: Surface<WindowSurface>,
    state: State,
    content: OverlayContent,
    alpha: f32,
    images: ImageCache,
}

/// The application driven by `EventLoop::run_app` on the overlay thread.
#[derive(Default)]
pub(crate) struct OverlayApp {
    backend: Option<Backend>,
    painter: Option<Painter>,
    egui_ctx: egui::Context,
    entries: HashMap<OverlayId, WindowEntry>,
    by_window: HashMap<WindowId, OverlayId>,
}

/// Start the overlay thread's event loop, handing the proxy to the facade.
pub(crate) fn run(proxy_tx: Sender<EventLoopProxy<LoopEvent>>) {
    let mut builder = EventLoop::<LoopEvent>::with_user_event();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // X11/Wayland refuse an event loop built off the main thread unless the
        // builder opts in.
        winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
    }
    #[cfg(windows)]
    {
        // The Windows backend panics on the same thing
        // (`windows/event_loop.rs:190`), so the overlay thread opts in the same
        // way: `EventLoopBuilderExtWindows::with_any_thread(true)`.
        winit::platform::windows::EventLoopBuilderExtWindows::with_any_thread(&mut builder, true);
    }
    let Ok(event_loop) = builder.build() else {
        eprintln!("overlay: could not create the winit event loop; overlays are unavailable");
        return;
    };
    if proxy_tx.send(event_loop.create_proxy()).is_err() {
        eprintln!("overlay: the overlay handle went away before the event loop started");
        return;
    }
    event_loop.set_control_flow(ControlFlow::Wait);
    if let Err(err) = event_loop.run_app(&mut OverlayApp::default()) {
        eprintln!("overlay: the event loop ended with an error: {err}");
    }
}

impl ApplicationHandler<LoopEvent> for OverlayApp {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        // Windows are created on demand from `LoopEvent::Create`, so there is
        // nothing to build here.
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: LoopEvent) {
        match event {
            LoopEvent::Create {
                id,
                spec,
                content,
                alpha,
                ack,
            } => {
                let result = self.create_window(event_loop, id, spec, content, alpha);
                if ack.send(result).is_err() {
                    eprintln!("overlay: the spawn handshake receiver is gone");
                }
            }
            LoopEvent::Redraw { id, content, alpha } => {
                if let Some(entry) = self.entries.get_mut(&id) {
                    entry.content = content;
                    entry.alpha = alpha;
                    entry.window.request_redraw();
                }
            }
            LoopEvent::Hide { id } => {
                self.close(id);
            }
            LoopEvent::QuitAll => {
                let ids: Vec<OverlayId> = self.entries.keys().copied().collect();
                for id in ids {
                    self.close(id);
                }
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::RedrawRequested => {
                if let Some(id) = self.by_window.get(&window_id).copied() {
                    self.paint(id);
                }
            }
            WindowEvent::CloseRequested => {
                if let Some(id) = self.by_window.get(&window_id).copied() {
                    self.close(id);
                    if self.entries.is_empty() {
                        event_loop.exit();
                    }
                }
            }
            event => {
                let Some(id) = self.by_window.get(&window_id).copied() else {
                    return;
                };
                let Some(entry) = self.entries.get_mut(&id) else {
                    return;
                };
                if entry.state.on_window_event(&entry.window, &event).repaint {
                    entry.window.request_redraw();
                }
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        // `Painter::destroy` needs the GL context bound to this thread.
        let Some(painter) = self.painter.as_mut() else {
            return;
        };
        if self
            .backend
            .as_ref()
            .is_some_and(|backend| backend.context_is_current())
        {
            painter.destroy();
        }
    }
}

impl OverlayApp {
    fn create_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: OverlayId,
        spec: OverlayWindowSpec,
        content: OverlayContent,
        alpha: f32,
    ) -> Result<(), OverlayError> {
        let pair = match &self.backend {
            Some(backend) => backend.create_window(event_loop, &spec)?,
            None => {
                let (backend, pair) = Backend::new(event_loop, &spec)?;
                self.backend = Some(backend);
                pair
            }
        };

        if self.painter.is_none() {
            let Some(backend) = &self.backend else {
                return Err(OverlayError::Gl {
                    message: "the GL backend vanished mid-setup".to_owned(),
                });
            };
            let painter = Painter::new(backend.glow(), "", None, false).map_err(|err| {
                OverlayError::Painter {
                    message: format!("egui_glow painter creation failed: {err}"),
                }
            })?;
            self.painter = Some(painter);
        }

        let WindowSurfacePair { window, surface } = pair;
        // Same Windows limitation as `host.rs`: `set_cursor_hittest(false)` makes
        // winit 0.30.13 add `WS_EX_TRANSPARENT | WS_EX_LAYERED` without ever
        // calling `SetLayeredWindowAttributes`/`UpdateLayeredWindow`, so the
        // unattributed layered window composites NOTHING. Click-through is
        // therefore skipped on Windows; the overlay keeps rendering (attributing
        // the layered window would need FFI, which lives only in `gl_boot.rs`).
        if spec.click_through
            && !cfg!(windows)
            && let Err(err) = window.set_cursor_hittest(false)
        {
            eprintln!("overlay: this platform cannot make the overlay click-through: {err}");
        }

        let viewport = ViewportId::from_hash_of(format!("virshot-overlay-{}", id.as_u64()));
        let Some(painter) = &self.painter else {
            return Err(OverlayError::Painter {
                message: "the painter vanished mid-setup".to_owned(),
            });
        };
        let state = State::new(
            self.egui_ctx.clone(),
            viewport,
            &window,
            Some(window.scale_factor() as f32),
            event_loop.system_theme(),
            Some(painter.max_texture_side()),
        );

        let window_id = window.id();
        self.entries.insert(
            id,
            WindowEntry {
                window,
                surface,
                state,
                content,
                alpha,
                images: ImageCache::new(),
            },
        );
        self.by_window.insert(window_id, id);

        // One paint so the window is not blank before the first `redraw`.
        self.paint(id);
        Ok(())
    }

    fn paint(&mut self, id: OverlayId) {
        let Some(backend) = &self.backend else {
            return;
        };
        let Some(painter) = self.painter.as_mut() else {
            return;
        };
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };

        if let Err(err) = backend.make_current(&entry.surface) {
            eprintln!("overlay: could not bind the GL context for {id}: {err}");
            return;
        }
        backend.resize(&entry.window, &entry.surface);

        let size = entry.window.inner_size();
        let dimensions = [size.width.max(1), size.height.max(1)];
        let input = entry.state.take_egui_input(&entry.window);
        let max_texture_side = painter.max_texture_side();
        let mut output = self.egui_ctx.run_ui(input, |ui| {
            if let Err(err) = paint(
                ui,
                &entry.content,
                entry.alpha,
                &mut entry.images,
                max_texture_side,
            ) {
                eprintln!("overlay: content painting failed for {id}: {err}");
            }
        });

        // Transparent overlays need fully transparent pixels behind the chip.
        painter.clear(dimensions, [0.0, 0.0, 0.0, 0.0]);

        // `TexturesDelta` is `Drop`, so its fields are taken rather than moved out.
        let uploads = std::mem::take(&mut output.textures_delta.set);
        for (texture_id, deltas) in uploads {
            for delta in deltas {
                painter.set_texture(texture_id, &delta);
            }
        }
        let clipped = self
            .egui_ctx
            .tessellate(output.shapes, output.pixels_per_point);
        painter.paint_primitives(dimensions, output.pixels_per_point, &clipped);
        let frees = std::mem::take(&mut output.textures_delta.free);
        for texture_id in frees {
            painter.free_texture(texture_id);
        }

        if let Err(err) = backend.swap_buffers(&entry.surface) {
            eprintln!("overlay: presenting {id} failed: {err}");
        }
        entry
            .state
            .handle_platform_output(&entry.window, output.platform_output);
    }

    fn close(&mut self, id: OverlayId) {
        let Some(entry) = self.entries.remove(&id) else {
            return;
        };
        self.by_window.remove(&entry.window.id());
    }
}
