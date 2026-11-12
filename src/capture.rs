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
//! `virshot shot` (Stage 5); `spawn_capture` is consumed by every trigger
//! source — the hotkey listener, the pad pump (Windows), and both tray paths —
//! so the semantics stay locked by unit tests (same precedent as `template`).
//!
//! Toast hand-off (spec §4.3, Stage 4 of the overlay/toast plan): every
//! outcome becomes a `toast::Toast` shown through the notification handle the
//! caller passes to `run_capture`/`spawn_capture` — success carries
//! `Saved: <file name>` (the full path stays in the log), failure carries the
//! localized chip wording of the typed error (catalog `toast.*`, spec §2; the
//! `Error` `Display` stays English for logs and headless CLI output). The
//! handle comes from the resident GUI's
//! `toast::ToastService` (painted by `crates/overlay`'s host backend) and is
//! read per trigger from [`ToastHandoff`]; a headless `virshot shot` run has
//! no GUI and therefore no handle (`None`), so the notification is skipped and
//! the CLI prints the path/logs instead. Which outcome produces a chip, and on
//! which monitor/corner/hold, is decided here by pure functions against
//! `config::Overlay` (the visibility gate from spec §4.3: errors always show,
//! success only while `overlay.enabled`).

use chrono::{DateTime, Local};
use image::ExtendedColorType;
use image::ImageEncoder;
use image::RgbaImage;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPEncoder;
use overlay::Corner;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use toast::MonitorSel;
use toast::Toast;
use toast::ToastColor;
use toast::ToastHandle;

use crate::config::Format;
use crate::config::General;
use crate::config::Overlay;
use crate::config::OverlayPosition;
use crate::config::Settings;
use crate::template;

/// Capture/save failures, typed per module (`thiserror` enum, no stringly
/// `Err(String)`). Each variant keeps its underlying error as `source`. The
/// `Display` strings are the English contract for logs and headless CLI
/// output (spec §2); the localized toast chip comes from [`chip_text`], not
/// from `Display`. `xcap` sources are boxed because `XCapError` is a fat
/// platform enum that would make the whole `Error` too large for `Result`.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// Capturing the monitor image failed; `reason` is the `xcap` error's
    /// message, kept as `source` whenever one was returned.
    #[error("failed to capture monitor image: {reason}")]
    MonitorCaptureFailed {
        reason: String,
        #[source]
        source: Option<Box<xcap::XCapError>>,
    },

    /// Reading the foreground window's title failed.
    #[error("failed to read window title: {reason}")]
    WindowTitleFailed {
        reason: String,
        #[source]
        source: Option<Box<xcap::XCapError>>,
    },

    /// Capturing the foreground window image failed.
    #[error("failed to capture window image: {reason}")]
    WindowCaptureFailed {
        reason: String,
        #[source]
        source: Option<Box<xcap::XCapError>>,
    },

    /// Window enumeration failed (no display server / unsupported backend).
    #[error("failed to list windows: {reason}")]
    WindowListFailed {
        reason: String,
        #[source]
        source: Option<Box<xcap::XCapError>>,
    },

    /// Monitor enumeration failed.
    #[error("failed to list monitors: {reason}")]
    MonitorListFailed {
        reason: String,
        #[source]
        source: Option<Box<xcap::XCapError>>,
    },

    /// No monitor was found at all when one was needed.
    #[error("no monitors found")]
    NoMonitorsFound,

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

/// Capture request: WHAT to capture — the mode plus the settings that decide
/// the target, format, and save path. The save thread takes ownership (spec
/// §4.2 thread boundary — no shared capture state, no channel back to the main
/// thread). WHERE the outcome is reported is a separate parameter (see
/// [`ToastHandoff`] and [`run_capture`]), not part of the request.
pub(crate) struct CaptureCommand {
    pub(crate) mode: CaptureMode,
    pub(crate) settings: Settings,
}

/// Latest-only slot holding the toast handle for the whole resident process
/// (spec §4.3 hand-off). The GUI's create closure attaches the handle once
/// `toast::ToastService` exists; trigger threads (hotkey listener, pad pump,
/// tray) read a clone per capture, so a capture started before the GUI is up —
/// or after it is gone — simply carries no handle. The same latest-only slot
/// of owned values the `hotkeys` settings mailbox uses, not shared capture
/// state.
pub(crate) type ToastHandoff = Arc<Mutex<Option<ToastHandle>>>;

/// Empty handoff: no toast service yet (or never, on the headless CLI path).
pub(crate) fn new_handoff() -> ToastHandoff {
    Arc::new(Mutex::new(None))
}

/// Attach the service's handle. A later attach replaces the earlier one — the
/// slot is latest-only, like every mailbox in this repo.
pub(crate) fn attach_handoff(handoff: &ToastHandoff, handle: ToastHandle) {
    let mut slot = handoff
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = Some(handle);
}

/// The handle a capture should use right now: `None` while no toast service is
/// attached. Poisoned (a panic elsewhere) is recovered, like every other
/// mailbox here — a toast is never worth aborting a capture over.
pub(crate) fn handoff_handle(handoff: &ToastHandoff) -> Option<ToastHandle> {
    handoff
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Visibility gate (spec §4.3): a success chip needs `overlay.enabled`; an
/// error chip shows ALWAYS — even with the overlay switched off, the user must
/// see why the capture failed.
fn should_show(outcome: &Result<PathBuf, Error>, overlay: &Overlay) -> bool {
    match outcome {
        Ok(_) => overlay.enabled,
        Err(_) => true,
    }
}

/// Configured corner → the chip corner (`crates/overlay` anchors by corner,
/// spec §4.3: the four screen corners, insets handled by `overlay::place`).
fn toast_corner(position: OverlayPosition) -> Corner {
    match position {
        OverlayPosition::TopLeft => Corner::TopLeft,
        OverlayPosition::TopRight => Corner::TopRight,
        OverlayPosition::BottomLeft => Corner::BottomLeft,
        OverlayPosition::BottomRight => Corner::BottomRight,
    }
}

/// Monitor selection (spec §4.3): `on_all_monitors` → one chip on every
/// monitor; otherwise the primary monitor, which the GUI puts FIRST in the
/// monitor list it hands the service, so `Index(0)` is that monitor.
fn monitor_sel(on_all_monitors: bool) -> MonitorSel {
    if on_all_monitors {
        MonitorSel::All
    } else {
        MonitorSel::Index(0)
    }
}

/// `duration_ms` from `[overlay]` as the chip's hold. The config field is
/// `u64`; a chip hold is `u32`, so an absurd value clamps instead of wrapping.
/// `0` stays `0` — sticky until dismissed (`crates/toast` rule).
fn toast_duration_ms(overlay: &Overlay) -> u32 {
    u32::try_from(overlay.duration_ms).unwrap_or(u32::MAX)
}

/// Success chip text (spec §4.3): `Saved: <file name>` only — the full path is
/// in the log; the wording comes from catalog `toast.saved` (spec §2). A path
/// with no usable file name shows the full path rather than an empty chip.
/// `pub(crate)` because the Overlay tab's test chip
/// (`gui::overlay::overlay_test_message`) builds the very same wording without a real
/// save, so the test button cannot drift from this format.
pub(crate) fn saved_text(path: &Path) -> String {
    match path.file_name() {
        Some(name) => rust_i18n::t!("toast.saved", file_name = name.to_string_lossy()).into_owned(),
        None => rust_i18n::t!("toast.saved", file_name = path.display()).into_owned(),
    }
}

/// Localized chip wording for a capture failure (spec §2): every variant maps
/// to a catalog `toast.*` key, so the chip text follows the UI locale while
/// the `Error` `Display` impl stays English for logs and CLI headless output.
/// The technical detail (`reason`) is the underlying error's English message —
/// only the localized prefix/template comes from the catalog; under `en` the
/// catalog values equal the `Display` strings byte-for-byte.
pub(crate) fn chip_text(err: &Error) -> String {
    match err {
        Error::NoActiveWindow => rust_i18n::t!("toast.no_active_window").into_owned(),
        Error::Template { source } => {
            rust_i18n::t!("toast.template_invalid", reason = source.to_string()).into_owned()
        }
        Error::Save { path, source } => rust_i18n::t!(
            "toast.save_failed",
            path = path.display(),
            reason = source.to_string()
        )
        .into_owned(),
        Error::Io { path, source } => rust_i18n::t!(
            "toast.dir_create_failed",
            path = path.display(),
            reason = source.to_string()
        )
        .into_owned(),
        Error::TooManyCollisions { path, limit } => rust_i18n::t!(
            "toast.too_many_collisions",
            path = path.display(),
            limit = limit
        )
        .into_owned(),
        Error::MonitorCaptureFailed { reason, .. } => {
            rust_i18n::t!("toast.monitor_capture_failed", reason = reason).into_owned()
        }
        Error::WindowTitleFailed { reason, .. } => {
            rust_i18n::t!("toast.window_title_failed", reason = reason).into_owned()
        }
        Error::WindowCaptureFailed { reason, .. } => {
            rust_i18n::t!("toast.window_capture_failed", reason = reason).into_owned()
        }
        Error::WindowListFailed { reason, .. } => {
            rust_i18n::t!("toast.window_list_failed", reason = reason).into_owned()
        }
        Error::MonitorListFailed { reason, .. } => {
            rust_i18n::t!("toast.monitor_list_failed", reason = reason).into_owned()
        }
        Error::NoMonitorsFound => rust_i18n::t!("toast.no_monitors_found").into_owned(),
    }
}

/// The chip for one capture outcome, or `None` when nothing is shown (success
/// with `overlay.enabled = false`). Success is a `Success` chip, failure a
/// `Danger` chip carrying the localized wording of the typed error ([`chip_text`],
/// spec §2) — `crates/toast` holds `Danger` for its own error hold regardless
/// of `duration_ms`.
fn toast_from_outcome(outcome: &Result<PathBuf, Error>, overlay: &Overlay) -> Option<Toast> {
    if !should_show(outcome, overlay) {
        return None;
    }
    let (text, color) = match outcome {
        Ok(path) => (saved_text(path), ToastColor::Success),
        Err(err) => (chip_text(err), ToastColor::Danger),
    };
    Some(Toast {
        monitor: monitor_sel(overlay.on_all_monitors),
        text,
        image: None,
        color,
        corner: toast_corner(overlay.position),
        duration_ms: toast_duration_ms(overlay),
    })
}

/// Show the outcome through the handle the caller passed (spec §4.3). No
/// handle (headless CLI, or a trigger before the GUI exists) or a suppressed
/// chip → nothing happens; a closed toast channel is WARNed, never fatal —
/// the capture result itself is what the caller gets.
fn notify(toast: Option<ToastHandle>, outcome: &Result<PathBuf, Error>, overlay: &Overlay) {
    let Some(handle) = toast else {
        tracing::debug!("No toast handle attached; notification skipped (headless run)");
        return;
    };
    let Some(toast) = toast_from_outcome(outcome, overlay) else {
        tracing::debug!("Toast suppressed: overlay disabled for a successful capture");
        return;
    };
    if let Err(err) = handle.show(toast) {
        tracing::warn!("Toast notification failed: {err}");
    }
}

/// Run one capture on a dedicated thread (spec §4.2: saving must not
/// micro-freeze the game). The thread stamps the capture moment at this
/// module boundary, grabs, encodes, saves, and logs the outcome: success is
/// `INFO "Saved: <full path>"` (the toast shows just the file name), failure
/// is `ERROR` with the typed error. The chip is shown by [`run_capture`] at
/// the module boundary, in the same frame as the log line. Nothing else is
/// shared back; the app never aborts on capture failure (the thread runs
/// detached). `toast` is the handle read from the shared hand-off slot by the
/// trigger (`handoff_handle`); `None` means no chip.
pub(crate) fn spawn_capture(command: CaptureCommand, toast: Option<ToastHandle>) {
    std::thread::spawn(move || {
        let now = chrono::Local::now();
        match run_capture(command, now, toast) {
            Ok(path) => tracing::info!("Saved: {}", path.display()),
            Err(err) => tracing::error!("Capture failed: {err}"),
        }
    });
}

/// Synchronous capture: run [`capture_core`] and show the outcome's toast
/// before returning it, so the user sees the same result the log records.
/// `toast` is the notification handle (`None` on the headless CLI path, where
/// the outcome is printed and logged instead — spec §4.3). Failure modes: see
/// [`Error`] — never a panic, callers log and keep running.
pub(crate) fn run_capture(
    command: CaptureCommand,
    now: DateTime<Local>,
    toast: Option<ToastHandle>,
) -> Result<PathBuf, Error> {
    // Destructured so the `[overlay]` section can be read after the settings
    // move into the capture core (no clone of the whole settings).
    let CaptureCommand { mode, settings } = command;
    let outcome = capture_core(mode, &settings, now);
    notify(toast, &outcome, &settings.overlay);
    outcome
}

/// Synchronous capture core: determine target → grab → collision-free path →
/// encode + save, returning the full destination path. `now` is injected (the
/// capture moment stamped by the caller at the module boundary), so filename
/// logic stays deterministic; `render` never reads the clock. Failure modes:
/// see [`Error`] — never a panic, callers log and keep running.
fn capture_core(
    mode: CaptureMode,
    settings: &Settings,
    now: DateTime<Local>,
) -> Result<PathBuf, Error> {
    let general = &settings.general;
    let ext = general.format.extension();

    let (image, title, template_str) = match mode {
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
            let image = monitor
                .capture_image()
                .map_err(|err| Error::MonitorCaptureFailed {
                    reason: err.to_string(),
                    source: Some(Box::new(err)),
                })?;
            // Full-screen template renders `fullscreen` for `%t` (template
            // module), so the target carries no title.
            (image, None, general.file_full_screen.as_str())
        }
        CaptureMode::Window => {
            let window = foreground_window()?.ok_or(Error::NoActiveWindow)?;
            let title = window.title().map_err(|err| Error::WindowTitleFailed {
                reason: err.to_string(),
                source: Some(Box::new(err)),
            })?;
            // Occlusion-independent: xcap uses PrintWindow on Windows.
            let image = window
                .capture_image()
                .map_err(|err| Error::WindowCaptureFailed {
                    reason: err.to_string(),
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
    let windows = xcap::Window::all().map_err(|err| Error::WindowListFailed {
        reason: err.to_string(),
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
    let monitors = xcap::Monitor::all().map_err(|err| Error::MonitorListFailed {
        reason: err.to_string(),
        source: Some(Box::new(err)),
    })?;
    if let Some(monitor) = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
    {
        return Ok(monitor.clone());
    }
    // `is_primary()` errored for every monitor: take the first, as before.
    monitors.into_iter().next().ok_or(Error::NoMonitorsFound)
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
