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
mod tests {
    use super::COLLISION_MAX_COUNTERS;
    use super::CaptureCommand;
    use super::CaptureMode;
    use super::Error;
    use super::WindowProbe;
    use super::encode_image;
    use super::pick_foreground;
    use super::run_capture;
    use super::save_image;
    use super::toast_notification;
    use super::unique_path;
    use crate::config::Format;
    use crate::config::General;
    use crate::config::Settings;
    use crate::overlay::Notification;
    use crate::overlay::new_mailbox;
    use crate::overlay::take_latest;
    use crate::template;
    use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
    use image::ImageFormat;
    use image::RgbaImage;
    use image::guess_format;
    use image::load_from_memory;
    use std::error::Error as _;
    use std::path::PathBuf;

    /// Fixed injected instant: 2026-11-03 00:12:05 local — the spec §4.2
    /// `%iso` example. Time is always injected; the module boundary stamps
    /// `Local::now()` itself, so tests never depend on the wall clock.
    fn fixed_now() -> DateTime<Local> {
        let naive = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
            NaiveTime::from_hms_opt(0, 12, 5).unwrap(),
        );
        Local.from_local_datetime(&naive).unwrap()
    }

    fn iso_stem() -> &'static str {
        "2026-11-03_00-12-05"
    }

    /// Scratch directory under `./.tmp/` (repo root, git-ignored), unique per
    /// test and removed when the test ends; relative so it behaves the same
    /// on linux and windows-gnu.
    struct ScratchDir {
        path: PathBuf,
    }

    impl ScratchDir {
        fn new(tag: &str) -> Self {
            let path = PathBuf::from("./.tmp").join(format!("capture-test-{tag}"));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("scratch dir must be creatable");
            Self { path }
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// Synthetic image with varying pixels so lossy codecs compress differently.
    fn synthetic_image(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([
                (x * 7 % 256) as u8,
                (y * 11 % 256) as u8,
                ((x + y) * 5 % 256) as u8,
                255,
            ])
        })
    }

    fn general_with(format: Format, jpeg_quality: u8, webp_quality: u8) -> General {
        General {
            format,
            jpeg_quality,
            webp_quality,
            ..General::default()
        }
    }

    // ---------- encoding ----------

    #[test]
    fn encode_image_png_roundtrip_decodes_lossless() {
        let image = synthetic_image(4, 4);
        let bytes = encode_image(&image, &general_with(Format::Png, 90, 90))
            .expect("png encoding must succeed");
        assert_eq!(
            guess_format(&bytes).expect("png magic must be recognized"),
            ImageFormat::Png
        );
        let decoded = load_from_memory(&bytes)
            .expect("png must decode")
            .to_rgba8();
        assert_eq!(decoded, image, "png is lossless");
    }

    #[test]
    fn encode_image_jpeg_quality_low_yields_smaller_file_than_high() {
        let image = synthetic_image(64, 64);
        let low = encode_image(&image, &general_with(Format::Jpeg, 10, 90))
            .expect("jpeg encoding at quality 10 must succeed");
        let high = encode_image(&image, &general_with(Format::Jpeg, 90, 90))
            .expect("jpeg encoding at quality 90 must succeed");
        assert!(
            low.len() < high.len(),
            "jpeg_quality must affect the output size: low={} high={}",
            low.len(),
            high.len()
        );
        assert_eq!(
            guess_format(&low).expect("jpeg magic must be recognized"),
            ImageFormat::Jpeg
        );
        let decoded = load_from_memory(&low).expect("jpeg must decode").to_rgba8();
        assert_eq!(
            decoded.dimensions(),
            (64, 64),
            "lossy decode must keep dimensions"
        );
    }

    #[test]
    fn encode_image_webp_roundtrip_decodes_valid_file() {
        let image = synthetic_image(4, 4);
        let bytes = encode_image(&image, &general_with(Format::Webp, 90, 50))
            .expect("webp encoding must succeed");
        assert_eq!(
            guess_format(&bytes).expect("webp magic must be recognized"),
            ImageFormat::WebP
        );
        let decoded = load_from_memory(&bytes)
            .expect("webp must decode")
            .to_rgba8();
        // image 0.25 ships image-webp with VP8L lossless only: pixels are exact,
        // `webp_quality` cannot affect the output yet (see `encode_image`).
        assert_eq!(decoded, image, "webp (VP8L) is lossless");
    }

    // ---------- foreground selection (pure probe logic) ----------

    fn probe(id: u32, pid: u32, focused: bool) -> WindowProbe {
        WindowProbe { id, pid, focused }
    }

    #[test]
    fn pick_foreground_own_pid_window_is_excluded() {
        let own_pid = std::process::id();
        let probes = [probe(1, own_pid, true), probe(2, 42, false)];
        assert_eq!(
            pick_foreground(&probes, own_pid),
            None,
            "own windows must never be the target"
        );
    }

    #[test]
    fn pick_foreground_focused_foreign_window_is_selected() {
        let own_pid = std::process::id();
        let probes = [
            probe(1, 11, false),
            probe(2, 22, true),
            probe(3, own_pid, true),
        ];
        assert_eq!(pick_foreground(&probes, own_pid), Some(1));
    }

    #[test]
    fn pick_foreground_no_focused_window_returns_none() {
        let probes = [probe(1, 11, false), probe(2, 22, false)];
        assert_eq!(pick_foreground(&probes, std::process::id()), None);
    }

    // ---------- collision scan ----------

    #[test]
    fn unique_path_empty_folder_returns_counter_one_path() {
        let dir = ScratchDir::new("unique-first");
        let path = unique_path(&dir.path, "%iso.%ext", fixed_now(), None, "png")
            .expect("first counter must be free in an empty folder");
        assert_eq!(path, dir.path.join(format!("{}.png", iso_stem())));
    }

    #[test]
    fn unique_path_existing_file_selects_variant_two_and_never_overwrites() {
        let dir = ScratchDir::new("unique-collision");
        let base = dir.path.join(format!("{}.png", iso_stem()));
        std::fs::write(&base, b"original-bytes").expect("fixture must be writable");
        let path = unique_path(&dir.path, "%iso.%ext", fixed_now(), None, "png")
            .expect("collision must resolve to a free variant");
        assert_eq!(path, dir.path.join(format!("{}-2.png", iso_stem())));
        assert!(!path.exists(), "chosen variant must not exist on disk");
        assert_eq!(
            std::fs::read(&base).expect("original file must survive"),
            b"original-bytes",
            "overwrite is forbidden"
        );
    }

    #[test]
    fn unique_path_n_token_template_re_renders_counter_two() {
        let dir = ScratchDir::new("unique-token-n");
        let first = dir.path.join(format!("{}-1.png", iso_stem()));
        std::fs::write(&first, b"original-bytes").expect("fixture must be writable");
        let path = unique_path(&dir.path, "%iso-%n.%ext", fixed_now(), None, "png")
            .expect("collision must re-render %n with counter 2");
        assert_eq!(path, dir.path.join(format!("{}-2.png", iso_stem())));
        assert_eq!(
            std::fs::read(&first).expect("original file must survive"),
            b"original-bytes",
            "overwrite is forbidden"
        );
    }

    #[test]
    fn unique_path_bound_exceeded_returns_too_many_collisions() {
        let dir = ScratchDir::new("unique-bound");
        std::fs::write(dir.path.join(format!("{}.png", iso_stem())), b"x")
            .expect("base fixture must be writable");
        for counter in 2..=COLLISION_MAX_COUNTERS {
            std::fs::write(dir.path.join(format!("{}-{counter}.png", iso_stem())), b"x")
                .expect("variant fixtures must be writable");
        }
        let err = unique_path(&dir.path, "%iso.%ext", fixed_now(), None, "png")
            .expect_err("bound must stop the scan");
        assert!(
            matches!(&err, Error::TooManyCollisions { path, limit }
                if *path == dir.path && *limit == COLLISION_MAX_COUNTERS),
            "expected TooManyCollisions for the folder, got {err:?}"
        );
        assert!(
            err.to_string().contains("1000"),
            "limit must be in the message: {err}"
        );
    }

    #[test]
    fn unique_path_absolute_template_maps_error_template() {
        let dir = ScratchDir::new("unique-absolute");
        let err = unique_path(&dir.path, "/abs/%iso.%ext", fixed_now(), None, "png")
            .expect_err("absolute template must fail validation");
        assert!(
            matches!(&err, Error::Template { .. }),
            "expected Error::Template, got {err:?}"
        );
        assert!(
            err.to_string().contains("/abs/%iso.%ext"),
            "template must be quoted: {err}"
        );
    }

    // ---------- save ----------

    #[test]
    fn save_image_png_creates_missing_subfolders_and_writes_decodable_file() {
        let dir = ScratchDir::new("save-subfolders");
        let path = dir.path.join("FullScreen/26-11/shot.png");
        save_image(
            &synthetic_image(4, 4),
            &general_with(Format::Png, 90, 90),
            &path,
        )
        .expect("save must create missing subfolders");
        assert!(path.exists(), "file must land at the template path");
        let decoded = image::open(&path)
            .expect("written png must decode")
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (4, 4));
    }

    #[test]
    fn save_image_parent_is_regular_file_returns_io_error() {
        let file = PathBuf::from("./.tmp/capture_save_parent_regular_file.png");
        std::fs::write(&file, b"not a directory").expect("scratch file must be writable");
        let err = save_image(
            &synthetic_image(2, 2),
            &general_with(Format::Png, 90, 90),
            &file.join("shot.png"),
        )
        .expect_err("regular file cannot be a parent directory");
        assert!(
            matches!(&err, Error::Io { path, .. } if *path == file),
            "expected Error::Io attributed to the parent, got {err:?}"
        );
        std::fs::remove_file(&file).expect("scratch file must be removable");
    }

    // ---------- typed errors ----------

    #[test]
    fn error_no_active_window_reports_spec_message_without_source() {
        let err = Error::NoActiveWindow;
        assert_eq!(err.to_string(), "no active window");
        assert!(err.source().is_none(), "nothing underneath this failure");
    }

    #[test]
    fn error_template_variant_keeps_template_error_as_source() {
        let err = Error::Template {
            source: template::Error::UnknownToken {
                token: "%month".to_owned(),
                template: "%month.%ext".to_owned(),
            },
        };
        assert!(
            err.to_string().contains("%month"),
            "token must be quoted: {err}"
        );
        assert!(
            err.source().is_some(),
            "template error must be kept as source"
        );
    }

    #[test]
    fn error_monitor_variant_with_source_displays_context_message() {
        let err = Error::Monitor {
            message: "failed to capture window image: Not supported".to_owned(),
            source: Some(Box::new(xcap::XCapError::NotSupported)),
        };
        assert_eq!(
            err.to_string(),
            "failed to capture window image: Not supported"
        );
        assert!(err.source().is_some(), "xcap error must be kept as source");
    }

    #[test]
    fn error_monitor_variant_without_source_reports_no_monitors() {
        let err = Error::Monitor {
            message: "no monitors found".to_owned(),
            source: None,
        };
        assert_eq!(err.to_string(), "no monitors found");
        assert!(err.source().is_none(), "no xcap error to report here");
    }

    #[test]
    fn error_save_variant_includes_destination_path_and_source() {
        let path = PathBuf::from(".tmp/capture-test-save/x.png");
        let err = Error::Save {
            path: path.clone(),
            source: image::ImageError::IoError(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "disk refused the write",
            )),
        };
        let msg = err.to_string();
        assert!(
            msg.contains(&path.display().to_string()),
            "destination path missing: {msg}"
        );
        assert!(
            msg.contains("disk refused the write"),
            "source message missing: {msg}"
        );
        assert!(err.source().is_some(), "image error must be kept as source");
    }

    #[test]
    fn error_io_variant_includes_path_and_source() {
        let err = Error::Io {
            path: PathBuf::from("./.tmp/capture-test-save/FullScreen"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "directory vanished"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("FullScreen"),
            "path missing from message: {msg}"
        );
        assert!(
            msg.contains("directory vanished"),
            "source message missing: {msg}"
        );
        assert!(err.source().is_some(), "io error must be kept as source");
    }

    // ---------- toast notification ----------

    #[test]
    fn toast_notification_success_carries_full_saved_path() {
        let path = PathBuf::from("./.tmp/capture-test-toast/shot.png");
        assert_eq!(
            toast_notification(&Ok(path.clone())),
            Notification::Success { path },
            "the toast hand-off keeps the full path (the log keeps it too)"
        );
    }

    #[test]
    fn toast_notification_error_carries_typed_message_for_toast_text() {
        let notification = toast_notification(&Err(Error::NoActiveWindow));
        assert_eq!(
            notification,
            Notification::Error {
                message: "no active window".to_owned()
            },
            "spec §4.2 requires exactly 'no active window' in the toast"
        );
    }

    /// Real-monitor smoke (spec §4.2): full-screen capture end to end. Kept
    /// out of the automated gate: the xcap linux backend enumerates monitors
    /// over XCB/X11, and this build host has no X11 socket (WSLg weston runs
    /// Wayland-only here), so the capture cannot run headless. Run manually
    /// on a host with a desktop session: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn run_capture_full_screen_writes_valid_file() {
        let dir = ScratchDir::new("smoke-fullscreen");
        let settings = Settings {
            general: General {
                save_path: dir.path.display().to_string(),
                ..General::default()
            },
            ..Settings::default()
        };
        let toast = new_mailbox();
        let path = run_capture(
            CaptureCommand {
                mode: CaptureMode::FullScreen,
                settings,
                toast: toast.clone(),
            },
            Local::now(),
        )
        .expect("full-screen capture must succeed on a host with a display");
        assert!(
            path.exists(),
            "capture must write a file at the template path"
        );
        let decoded = image::open(&path).expect("written file must decode");
        assert!(
            decoded.width() > 0 && decoded.height() > 0,
            "captured image must not be empty"
        );
        assert_eq!(
            take_latest(&toast),
            Some(Notification::Success { path }),
            "a successful capture must hand the saved path to the toast"
        );
    }
}
