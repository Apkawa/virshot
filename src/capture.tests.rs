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
