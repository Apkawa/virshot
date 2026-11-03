//! Capture module (spec §4.2): determines the capture target from the
//! FOREGROUND window at trigger time, grabs the image, encodes it to the
//! configured format (`png`/`jpg`/`webp`), and saves it under a template
//! path with collision handling — on a separate thread so saving never
//! micro-freezes the game.
//!
//! Foreground analysis excludes the app's own windows (settings/overlay):
//! windows whose pid equals `std::process::id()` are skipped. Full-screen
//! mode captures the monitor containing the foreground window and falls back
//! to the primary monitor when the foreground cannot be determined. Window
//! mode captures the foreground window regardless of occlusion (`xcap` uses
//! PrintWindow internally on Windows); with no focused foreign window the
//! desktop is visible → typed `NoActiveWindow`, nothing is saved.
//!
//! Collisions are resolved by scanning the save folder on disk (no state
//! file): overwrite is forbidden, the counter increments (`%n` re-render or
//! `template::collision_variant`) until the path is free.
//!
//! Wiring: the CLI (`main.rs`) consumes `run_capture` for headless
//! `virshot shot` (Stage 5); `spawn_capture` is consumed by the hotkey wiring
//! in Stage 6, so only that fn carries an `allow(dead_code)` meanwhile — the
//! semantics stay locked by unit tests (same precedent as `template`).
//!
//! Toast hand-off (spec §4.3, Stage 9): every outcome is also pushed to the
//! command's `overlay::ToastMailbox` — success carries the full saved path,
//! failure carries the typed error's message. The mailbox is a latest-only
//! slot of owned values (the same permitted pattern as the `hotkeys` settings
//! mailbox), not shared capture state; a headless run passes a mailbox nobody
//! polls, so the push is inert there.

use chrono::{DateTime, Local};
use image::ExtendedColorType;
use image::ImageEncoder;
use image::RgbaImage;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use std::path::Path;
use std::path::PathBuf;

use crate::config::Format;
use crate::config::General;
use crate::config::Settings;
use crate::overlay::Notification;
use crate::overlay::ToastMailbox;
use crate::overlay::push;
use crate::template;

/// Capture/save failures, typed per module (`thiserror` enum, no stringly
/// `Err(String)`). Each variant keeps its underlying error as `source`.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// Monitor enumeration, monitor capture, window enumeration, or window
    /// capture failed. Context lives in `message`; the `xcap` error is kept
    /// as `source` whenever one was returned. Boxed because `XCapError` is a
    /// fat platform enum that would make the whole `Error` too large for
    /// `Result`.
    #[error("{message}")]
    Monitor {
        message: String,
        #[source]
        source: Option<Box<xcap::XCapError>>,
    },

    /// Window mode with no focused foreign window (the desktop is visible):
    /// spec §4.2 requires exactly "no active window" in toast + log, and no
    /// file is saved.
    #[error("no active window")]
    NoActiveWindow,

    /// The filename template was rejected at render time (absolute path,
    /// `..` segment, unknown token) — the `template` error is kept as
    /// `source` and its message already quotes the offending template.
    #[error("invalid filename template: {source}")]
    Template {
        #[source]
        source: template::Error,
    },

    /// Encoding or writing failed; `path` is the destination file and the
    /// `image` error is kept as `source` (a write failure arrives wrapped as
    /// `ImageError::IoError`).
    #[error("failed to save {}: {source}", path.display())]
    Save {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },

    /// Filesystem failure (creating missing template subfolders), attributed
    /// to `path` with the `io::Error` kept as `source`.
    #[error("failed to create output directory {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The collision scan found an existing file for every counter up to
    /// [`COLLISION_MAX_COUNTERS`]; nothing was saved. `path` is the save
    /// folder that was scanned.
    #[error("too many filename collisions in {} after {limit} attempts", path.display())]
    TooManyCollisions { path: PathBuf, limit: u32 },
}

/// Capture target selected by the trigger binding (spec §4.1/§4.2); the
/// reserved rect-area action is out of MVP scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CaptureMode {
    /// Capture the monitor containing the foreground window (primary fallback).
    FullScreen,
    /// Capture the foreground window itself.
    Window,
}

/// Owned values for one capture: mode + settings + the toast mailbox. The save
/// thread takes ownership (spec §4.2 thread boundary — no shared capture
/// state, no channel back to the main thread); the mailbox is the one
/// latest-only hand-off, consumed by the GUI overlay (spec §4.3).
#[derive(Debug, Clone)]
pub(crate) struct CaptureCommand {
    pub(crate) mode: CaptureMode,
    pub(crate) settings: Settings,
    pub(crate) toast: ToastMailbox,
}

/// The toast notification for one capture outcome (spec §4.3): success carries
/// the full saved path (`overlay::toast_text` shows just the file name, the
/// log keeps the full path), failure carries the typed error's display
/// message.
fn toast_notification(outcome: &Result<PathBuf, Error>) -> Notification {
    match outcome {
        Ok(path) => Notification::Success { path: path.clone() },
        Err(err) => Notification::Error {
            message: err.to_string(),
        },
    }
}

/// Run one capture on a dedicated thread (spec §4.2: saving must not
/// micro-freeze the game). The thread stamps the capture moment at this
/// module boundary, grabs, encodes, saves, and logs the outcome: success is
/// `INFO "Saved: <full path>"` (the toast shows just the file name), failure
/// is `ERROR` with the typed error. The toast notification is pushed by
/// [`run_capture`] at the module boundary, in the same frame as the log line.
/// Nothing else is shared back; the app never aborts on capture failure (the
/// handle is dropped, the thread runs detached).
#[allow(dead_code)] // consumed by the hotkey wiring in Stage 6; headless `shot` uses `run_capture`
pub(crate) fn spawn_capture(command: CaptureCommand) {
    std::thread::spawn(move || {
        let now = chrono::Local::now();
        match run_capture(command, now) {
            Ok(path) => tracing::info!("Saved: {}", path.display()),
            Err(err) => tracing::error!("Capture failed: {err}"),
        }
    });
}

/// Synchronous capture: run [`capture_core`] and hand the outcome to the
/// command's toast mailbox before returning it, so the GUI sees the same
/// result the log records. Failure modes: see [`Error`] — never a panic,
/// callers log and keep running.
pub(crate) fn run_capture(command: CaptureCommand, now: DateTime<Local>) -> Result<PathBuf, Error> {
    let toast = command.toast.clone();
    let outcome = capture_core(command, now);
    push(&toast, toast_notification(&outcome));
    outcome
}

/// Synchronous capture core: determine target → grab → collision-free path →
/// encode + save, returning the full destination path. `now` is injected (the
/// capture moment stamped by the caller at the module boundary), so filename
/// logic stays deterministic; `render` never reads the clock. Failure modes:
/// see [`Error`] — never a panic, callers log and keep running.
fn capture_core(command: CaptureCommand, now: DateTime<Local>) -> Result<PathBuf, Error> {
    let general = &command.settings.general;
    let ext = general.format.extension();

    let (image, title, template_str) = match command.mode {
        CaptureMode::FullScreen => {
            // Foreground cannot be determined (no focused foreign window, or
            // enumeration failed on a host without the platform's window API)
            // → spec §4.2 fallback to the primary monitor; keep the reason in
            // the log.
            let foreground = match foreground_window() {
                Ok(window) => window,
                Err(err) => {
                    tracing::warn!("foreground window unavailable: {err}; using primary monitor");
                    None
                }
            };
            let monitor = monitor_for_full_screen(foreground.as_ref())?;
            let image = monitor.capture_image().map_err(|err| Error::Monitor {
                message: format!("failed to capture monitor image: {err}"),
                source: Some(Box::new(err)),
            })?;
            // Full-screen template renders `fullscreen` for `%t` (template
            // module), so the target carries no title.
            (image, None, general.file_full_screen.as_str())
        }
        CaptureMode::Window => {
            let window = foreground_window()?.ok_or(Error::NoActiveWindow)?;
            let title = window.title().map_err(|err| Error::Monitor {
                message: format!("failed to read window title: {err}"),
                source: Some(Box::new(err)),
            })?;
            // Occlusion-independent: xcap uses PrintWindow on Windows.
            let image = window.capture_image().map_err(|err| Error::Monitor {
                message: format!("failed to capture window image: {err}"),
                source: Some(Box::new(err)),
            })?;
            (image, Some(title), general.file_window.as_str())
        }
    };

    let path = unique_path(
        Path::new(&general.save_path),
        template_str,
        now,
        title.as_deref(),
        ext,
    )?;
    save_image(&image, general, &path)?;
    Ok(path)
}

/// Per-window facts for foreground selection, extracted from `xcap` without
/// `xcap` types so the selection rules stay unit-testable without a display.
/// `pid` is 0 when the window exposes none (it then counts as foreign);
/// `focused` is false when the platform cannot answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowProbe {
    id: u32,
    pid: u32,
    focused: bool,
}

/// Pick the foreground window among `probes`: the first focused window whose
/// pid is not the app's own pid (spec §4.2 excludes settings/overlay windows
/// from foreground analysis). None = no focused foreign window (desktop).
fn pick_foreground(probes: &[WindowProbe], own_pid: u32) -> Option<usize> {
    probes
        .iter()
        .position(|probe| probe.pid != own_pid && probe.focused)
}

/// The foreground window at trigger time, or None when the desktop is
/// visible. Err means window enumeration failed (no display server /
/// unsupported backend): callers fall back where spec §4.2 allows
/// (full-screen → primary monitor) or report it (window mode keeps the
/// typed error).
///
/// Platform notes (verified against xcap 0.9.8 source): Windows
/// `is_focused()` is `GetForegroundWindow() == hwnd`; linux/X11 reads
/// `_NET_ACTIVE_WINDOW` from the WM root window over XCB — XWayland clients
/// only, so on Wayland-only compositors foreground detection is best-effort
/// and the full-screen primary fallback covers it. `Window::all()` returns
/// windows z-ordered on both platforms.
fn foreground_window() -> Result<Option<xcap::Window>, Error> {
    let windows = xcap::Window::all().map_err(|err| Error::Monitor {
        message: format!("failed to list windows: {err}"),
        source: Some(Box::new(err)),
    })?;
    let probes: Vec<WindowProbe> = windows
        .iter()
        .map(|window| WindowProbe {
            id: window.id().unwrap_or(0),
            pid: window.pid().unwrap_or(0),
            focused: window.is_focused().unwrap_or(false),
        })
        .collect();
    Ok(pick_foreground(&probes, std::process::id()).map(|index| windows[index].clone()))
}

/// Monitor for full-screen capture: the monitor containing the foreground
/// window; when there is no foreground window or its monitor lookup fails,
/// fall back to the primary monitor (the existing PoC logic stays as the
/// fallback path).
fn monitor_for_full_screen(foreground: Option<&xcap::Window>) -> Result<xcap::Monitor, Error> {
    if let Some(window) = foreground {
        match window.current_monitor() {
            Ok(monitor) => return Ok(monitor),
            Err(err) => {
                tracing::warn!(
                    "foreground window monitor unavailable: {err}; using primary monitor"
                );
            }
        }
    }
    let monitors = xcap::Monitor::all().map_err(|err| Error::Monitor {
        message: format!("failed to list monitors: {err}"),
        source: Some(Box::new(err)),
    })?;
    if let Some(monitor) = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
    {
        return Ok(monitor.clone());
    }
    // `is_primary()` errored for every monitor: take the first, as before.
    monitors.into_iter().next().ok_or_else(|| Error::Monitor {
        message: "no monitors found".to_owned(),
        source: None,
    })
}

/// Upper bound for the collision scan (spec §4.2 counter suffix): reaching
/// it means the folder is packed with `-N` variants → typed error, nothing
/// saved.
const COLLISION_MAX_COUNTERS: u32 = 1000;

/// Pick a collision-free save path under `save_path` (overwrite forbidden):
/// render with counter 1; while the file exists, bump the counter — a
/// template containing `%n` is re-rendered, any other template gets
/// `template::collision_variant` (`-N` before the extension). Existence is
/// checked against the disk, no state file; best-effort scan (a racing
/// writer between check and write is not detected). Missing subfolders are
/// created by [`save_image`].
fn unique_path(
    save_path: &Path,
    template_str: &str,
    now: DateTime<Local>,
    title: Option<&str>,
    ext: &str,
) -> Result<PathBuf, Error> {
    let base = render_relative(template_str, now, title, ext, 1)?;
    let uses_counter_token = template_str.contains("%n");
    for counter in 1..=COLLISION_MAX_COUNTERS {
        let relative = if uses_counter_token {
            render_relative(template_str, now, title, ext, counter)?
        } else {
            template::collision_variant(&base, counter)
        };
        let path = save_path.join(relative);
        if !path.exists() {
            return Ok(path);
        }
    }
    Err(Error::TooManyCollisions {
        path: save_path.to_path_buf(),
        limit: COLLISION_MAX_COUNTERS,
    })
}

/// `template::render` with the capture error type; the rendered path is
/// always `save_path`-relative.
fn render_relative(
    template_str: &str,
    now: DateTime<Local>,
    title: Option<&str>,
    ext: &str,
    counter: u32,
) -> Result<String, Error> {
    template::render(template_str, now, title, ext, counter)
        .map_err(|err| Error::Template { source: err })
}

/// Encode `image` to `path` in the configured format, creating the missing
/// template subfolders first (spec §4.2). Directory creation maps to `Io`;
/// encode and write failures map to `Save`.
fn save_image(image: &RgbaImage, general: &General, path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| Error::Io {
            path: parent.to_path_buf(),
            source: err,
        })?;
    }
    let bytes = encode_image(image, general).map_err(|err| Error::Save {
        path: path.to_path_buf(),
        source: err,
    })?;
    std::fs::write(path, &bytes).map_err(|err| Error::Save {
        path: path.to_path_buf(),
        source: image::ImageError::IoError(err),
    })?;
    Ok(())
}

/// Encode to an in-memory buffer in the configured format: png lossless,
/// jpg lossy with `jpeg_quality`, webp lossless — `image` 0.25 ships
/// image-webp with VP8L lossless only, so `webp_quality` cannot affect the
/// output yet; lossy WebP needs libwebp or the `wp` crate, a new dependency
/// pending user approval (spec §4.2 lossy webp deferred, format still saves
/// as `.webp`). jpeg carries no alpha, so the alpha channel is dropped and
/// RGB8 is encoded (the jpeg encoder rejects `Rgba8`).
fn encode_image(image: &RgbaImage, general: &General) -> Result<Vec<u8>, image::ImageError> {
    let mut buf = Vec::new();
    let (width, height) = (image.width(), image.height());
    match general.format {
        Format::Png => PngEncoder::new(&mut buf).write_image(
            image.as_raw(),
            width,
            height,
            ExtendedColorType::Rgba8,
        )?,
        Format::Jpeg => JpegEncoder::new_with_quality(&mut buf, general.jpeg_quality).write_image(
            &rgb_bytes(image),
            width,
            height,
            ExtendedColorType::Rgb8,
        )?,
        Format::Webp => WebPEncoder::new_lossless(&mut buf).write_image(
            image.as_raw(),
            width,
            height,
            ExtendedColorType::Rgba8,
        )?,
    }
    Ok(buf)
}

/// RGB8 bytes with the alpha channel dropped (jpeg has no alpha).
fn rgb_bytes(image: &RgbaImage) -> Vec<u8> {
    image
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
        .collect()
}

#[cfg(test)]
#[path = "capture.tests.rs"]
mod tests;
