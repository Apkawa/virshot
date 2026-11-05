//! OpenGL bootstrap for overlay windows: one display, config, and context per
//! event-loop thread, plus a window and window surface per overlay.
//!
//! The layout mirrors `eframe-0.37.0/src/native/glow_integration.rs`: the GL
//! context is created once (against the first window's handle) and every later
//! overlay window gets its own surface on that same context, so a thread can
//! paint several click-through windows by making each surface current.
//!
//! Deviation from the "no unsafe" rule: glutin 0.32.3 / glutin-winit 0.5.0 only
//! expose `unsafe fn` for config selection, context creation, window-surface
//! creation, and `glow::Context::from_loader_function_cstr` is unsafe. There is
//! no safe wrapper anywhere in those crates, so this crate root uses
//! `#![deny(unsafe_code)]` and confines every `unsafe` block to this module, the
//! same shape `crates/gamepad` uses (`#![deny(unsafe_code)]` at the root with a
//! module-scoped `#![allow(unsafe_code)]`). All `unsafe` calls here are direct
//! FFI into the platform GL API, exactly as eframe does.
#![allow(unsafe_code)] // glutin's context/surface creation APIs are `unsafe fn` only.

use std::ffi::CStr;
use std::sync::Arc;

use glutin::config::{Config, ConfigTemplateBuilder, GlConfig};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentContext, NotCurrentGlContext,
    PossiblyCurrentContext, PossiblyCurrentGlContext,
};
use glutin::display::{Display, GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use glutin_winit::{ApiPreference, DisplayBuilder, GlWindow};
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::HasWindowHandle as _; // winit re-exports raw-window-handle 0.6.
use winit::window::{Window, WindowAttributes, WindowLevel};

use crate::OverlayWindowSpec;
use crate::error::OverlayError;

/// A window together with the GL surface that paints it.
pub(crate) struct WindowSurfacePair {
    pub(crate) window: Window,
    pub(crate) surface: Surface<WindowSurface>,
}

/// Display, config, and context shared by every overlay window on one thread.
pub(crate) struct Backend {
    config: Config,
    context: PossiblyCurrentContext,
    glow: Arc<glow::Context>,
}

impl Backend {
    /// Create the first overlay window plus the display, config, and context.
    ///
    /// Panics only if the display offers zero GL configurations (startup init);
    /// every other failure is returned as `OverlayError`.
    pub(crate) fn new(
        event_loop: &ActiveEventLoop,
        spec: &OverlayWindowSpec,
    ) -> Result<(Backend, WindowSurfacePair), OverlayError> {
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(spec.transparent)
            .prefer_hardware_accelerated(None);

        let (window, config) = DisplayBuilder::new()
            .with_preference(ApiPreference::FallbackEgl)
            .with_window_attributes(Some(window_attributes(spec)))
            .build(event_loop, template, |mut configs| {
                configs.next().unwrap_or_else(|| {
                    panic!("this display offers no OpenGL configuration for the overlay window")
                })
            })
            .map_err(|err| OverlayError::Gl {
                message: format!("config/window selection failed: {err}"),
            })?;

        let Some(window) = window else {
            return Err(OverlayError::Window {
                message: "display builder returned no window".to_owned(),
            });
        };

        let display = config.display();
        if spec.transparent && config.supports_transparency() == Some(false) && !cfg!(windows) {
            eprintln!(
                "overlay: the GL config does not support transparency; the window may be opaque"
            );
        }

        let context = create_context(&display, &config, &window)?;
        let surface = create_surface(&display, &config, &window)?;
        let context = context
            .make_current(&surface)
            .map_err(|err| OverlayError::Gl {
                message: format!("could not make the overlay context current: {err}"),
            })?;
        set_swap_interval(&surface, &context);

        let glow = unsafe {
            Arc::new(glow::Context::from_loader_function_cstr(|name: &CStr| {
                display.get_proc_address(name)
            }))
        };

        Ok((
            Backend {
                config,
                context,
                glow,
            },
            WindowSurfacePair { window, surface },
        ))
    }

    /// Create an additional overlay window sharing this backend's context.
    pub(crate) fn create_window(
        &self,
        event_loop: &ActiveEventLoop,
        spec: &OverlayWindowSpec,
    ) -> Result<WindowSurfacePair, OverlayError> {
        let attributes = window_attributes(spec);
        if spec.transparent && self.config.supports_transparency() == Some(false) && !cfg!(windows)
        {
            eprintln!(
                "overlay: the GL config does not support transparency; the window may be opaque"
            );
        }
        // eframe keeps `create_window` for transparent Windows windows because
        // some GL paths report no transparency support although composition
        // works; everywhere else the config-aware finalization applies.
        let window = if cfg!(windows) && spec.transparent {
            event_loop
                .create_window(attributes)
                .map_err(|err| OverlayError::Window {
                    message: format!("could not create the overlay window: {err}"),
                })?
        } else {
            glutin_winit::finalize_window(event_loop, attributes, &self.config).map_err(|err| {
                OverlayError::Window {
                    message: format!("could not create the overlay window: {err}"),
                }
            })?
        };

        let display = self.config.display();
        let surface = create_surface(&display, &self.config, &window)?;
        self.context
            .make_current(&surface)
            .map_err(|err| OverlayError::Gl {
                message: format!("could not make the overlay context current: {err}"),
            })?;
        set_swap_interval(&surface, &self.context);

        Ok(WindowSurfacePair { window, surface })
    }

    /// Handle for creating the `egui_glow` painter.
    pub(crate) fn glow(&self) -> Arc<glow::Context> {
        Arc::clone(&self.glow)
    }

    /// Bind this backend's context to `surface` on the calling thread.
    pub(crate) fn make_current(
        &self,
        surface: &Surface<WindowSurface>,
    ) -> Result<(), OverlayError> {
        self.context
            .make_current(surface)
            .map_err(|err| OverlayError::Gl {
                message: format!("could not make the overlay context current: {err}"),
            })
    }

    /// True when the context is bound to the calling thread, as `Painter::destroy` requires.
    pub(crate) fn context_is_current(&self) -> bool {
        self.context.is_current()
    }

    /// Match the surface to the window's current size.
    pub(crate) fn resize(&self, window: &Window, surface: &Surface<WindowSurface>) {
        GlWindow::resize_surface(window, surface, &self.context);
    }

    /// Present the painted frame.
    pub(crate) fn swap_buffers(
        &self,
        surface: &Surface<WindowSurface>,
    ) -> Result<(), OverlayError> {
        surface
            .swap_buffers(&self.context)
            .map_err(|err| OverlayError::Gl {
                message: format!("swap_buffers failed: {err}"),
            })
    }
}

fn window_attributes(spec: &OverlayWindowSpec) -> WindowAttributes {
    let attributes = WindowAttributes::default()
        .with_title("virshot overlay")
        .with_inner_size(winit::dpi::PhysicalSize::new(
            spec.rect.w.round().max(1.0) as u32,
            spec.rect.h.round().max(1.0) as u32,
        ))
        .with_position(winit::dpi::PhysicalPosition::new(spec.rect.x, spec.rect.y))
        .with_transparent(spec.transparent)
        .with_decorations(false)
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_content_protected(spec.content_protected)
        .with_resizable(false)
        .with_visible(true);
    // Overlay windows are non-interactive helpers, so they stay invisible
    // in the Windows taskbar; other OSes have no taskbar knob here.
    #[cfg(windows)]
    let attributes = {
        use winit::platform::windows::WindowAttributesExtWindows as _;
        attributes.with_skip_taskbar(true)
    };
    attributes
}

/// Mirrors eframe's `context_attributes_to_try`: default, then GLES, then the
/// compatibility profile, first success wins. The window's raw handle is passed
/// (as eframe does, `glow_integration.rs:1268-1276`): on WGL Microsoft requires
/// the pixel format to be set on the DC before `wglCreateContext`/
/// `wglCreateContextAttribsARB`, and glutin only applies it
/// (`config.apply_on_native_window` → `SetPixelFormat`) when the attributes carry
/// the handle (`api/wgl/context.rs:35-41`, `api/wgl/config.rs:295-311`; glutin
/// docs: "WGL: you must pass a raw_window_handle"). EGL/GLX context creation
/// never reads the handle (`api/egl/context.rs`, `api/glx/context.rs`), so
/// passing it cannot regress those paths.
fn create_context(
    display: &Display,
    config: &Config,
    window: &Window,
) -> Result<NotCurrentContext, OverlayError> {
    let raw_window_handle = window
        .window_handle()
        .map(|handle| handle.as_raw())
        .map_err(|err| OverlayError::Window {
            message: format!("window handle unavailable for the overlay context: {err}"),
        })?;
    let mut last_error = String::new();
    for builder in [
        ContextAttributesBuilder::new(),
        ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(None)),
        ContextAttributesBuilder::new().with_profile(GlProfile::Compatibility),
    ] {
        let attributes = builder.build(Some(raw_window_handle));
        match unsafe { display.create_context(config, &attributes) } {
            Ok(context) => return Ok(context),
            Err(err) => {
                last_error = format!("{err}");
                eprintln!("overlay: GL context creation attempt failed: {err}");
            }
        }
    }
    Err(OverlayError::Gl {
        message: format!("could not create a GL context for the overlay: {last_error}"),
    })
}

fn create_surface(
    display: &Display,
    config: &Config,
    window: &Window,
) -> Result<Surface<WindowSurface>, OverlayError> {
    let attributes = window
        .build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())
        .map_err(|err| OverlayError::Window {
            message: format!("window handle unavailable for the overlay surface: {err}"),
        })?;
    unsafe {
        display
            .create_window_surface(config, &attributes)
            .map_err(|err| OverlayError::Gl {
                message: format!("could not create the overlay surface: {err}"),
            })
    }
}

/// Overlays repaint on demand, so presents are not synchronized to a video
/// frame: `SwapInterval::Wait` can block forever on a headless display that
/// never delivers vblank events.
fn set_swap_interval(surface: &Surface<WindowSurface>, context: &PossiblyCurrentContext) {
    if let Err(err) = surface.set_swap_interval(context, SwapInterval::DontWait) {
        eprintln!("overlay: could not set the overlay swap interval: {err}");
    }
}
