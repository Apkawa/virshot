use super::COLLISION_MAX_COUNTERS;
use super::CaptureCommand;
use super::CaptureMode;
use super::Error;
use super::WindowProbe;
use super::attach_handoff;
use super::chip_text;
use super::encode_image;
use super::handoff_handle;
use super::new_handoff;
use super::pick_foreground;
use super::run_capture;
use super::save_image;
use super::saved_text;
use super::should_show;
use super::toast_duration_ms;
use super::toast_from_outcome;
use super::unique_path;
use crate::config::Format;
use crate::config::General;
use crate::config::Overlay;
use crate::config::OverlayPosition;
use crate::config::Settings;
use crate::template;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use image::ImageFormat;
use image::RgbaImage;
use image::guess_format;
use image::load_from_memory;
use overlay::Corner;
use overlay::OverlayHost;
use std::error::Error as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::MutexGuard;
use toast::MonitorSel;
use toast::ToastColor;
use toast::ToastService;

/// Take the crate-wide locale lock (`crate::LOCALE_LOCK`): the locale and its
/// cache version are process-global, so locale-touching tests in every file
/// serialize on this one lock, not on a per-file one. A poisoned lock (a panic
/// in another locale test) is recovered rather than unwrapped, like every
/// other mailbox in this crate.
fn locale_guard() -> MutexGuard<'static, ()> {
    crate::LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

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
    // Foreign pids are offsets of the test process pid: hardcoded small pids
    // (11, 22) can collide with the process pid and make the test flaky.
    let probes = [
        probe(1, own_pid.wrapping_add(1), false),
        probe(2, own_pid.wrapping_add(2), true),
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
    // ScratchDir creates `./.tmp/` itself: a CI checkout has no `./.tmp/`, so a
    // bare `./.tmp/...` path would fail with NotFound before the assertion runs.
    let dir = ScratchDir::new("save-parent-file");
    let file = dir.path.join("capture_save_parent_regular_file.png");
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
fn error_window_capture_failed_with_source_displays_context_message() {
    let err = Error::WindowCaptureFailed {
        reason: "Not supported".to_owned(),
        source: Some(Box::new(xcap::XCapError::NotSupported)),
    };
    assert_eq!(
        err.to_string(),
        "failed to capture window image: Not supported"
    );
    assert!(err.source().is_some(), "xcap error must be kept as source");
}

#[test]
fn error_no_monitors_found_reports_no_monitors() {
    let err = Error::NoMonitorsFound;
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

/// Spec §2: `capture::Error`'s `Display` is the English contract for logs and
/// headless CLI output — it never consults the catalog, so switching the UI
/// locale must not localize it (the chip path is separate, see `chip_text`).
#[test]
fn error_display_stays_english_under_russian_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let no_window = Error::NoActiveWindow.to_string();
    let save = Error::Save {
        path: PathBuf::from("./.tmp/capture-test-locale/x.png"),
        source: image::ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "disk refused the write",
        )),
    }
    .to_string();
    let io = Error::Io {
        path: PathBuf::from("./.tmp/capture-test-locale/FullScreen"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "directory vanished"),
    }
    .to_string();

    // Restore BEFORE the assertions so a failure never leaves `ru` installed.
    rust_i18n::set_locale("en");

    assert_eq!(
        no_window, "no active window",
        "Display must stay English under the ru locale"
    );
    assert!(
        save.contains("failed to save"),
        "Save Display must keep the English prefix: {save}"
    );
    assert!(
        io.contains("failed to create output directory"),
        "Io Display must keep the English prefix: {io}"
    );
}

// ---------- toast chip from outcome (spec §4.3) ----------

/// `[overlay]` section with explicit values — the chip mapping reads exactly
/// these four keys (spec §7).
fn overlay(
    position: OverlayPosition,
    enabled: bool,
    on_all_monitors: bool,
    duration_ms: u64,
) -> Overlay {
    Overlay {
        enabled,
        position,
        duration_ms,
        on_all_monitors,
    }
}

/// The typed failure whose message the chip must carry verbatim (spec §4.2:
/// exactly "no active window").
fn capture_failure() -> Error {
    Error::NoActiveWindow
}

#[test]
fn should_show_success_only_while_overlay_enabled() {
    let ok: Result<PathBuf, Error> = Ok(PathBuf::from("./.tmp/shot.png"));
    assert!(
        should_show(
            &ok,
            &overlay(OverlayPosition::BottomRight, true, true, 2000)
        ),
        "a success chip needs overlay.enabled"
    );
    assert!(
        !should_show(
            &ok,
            &overlay(OverlayPosition::BottomRight, false, true, 2000)
        ),
        "a success chip is suppressed when overlay.enabled is false"
    );
}

#[test]
fn should_show_error_even_with_overlay_disabled() {
    assert!(
        should_show(
            &Err(capture_failure()),
            &overlay(OverlayPosition::BottomRight, false, false, 2000)
        ),
        "spec §4.3: errors show even with the overlay switched off"
    );
}

#[test]
fn saved_text_shows_the_file_name_only() {
    assert_eq!(
        saved_text(&PathBuf::from("./.tmp/capture-test-toast/shot.png")),
        "Saved: shot.png",
        "the chip names the file; the full path stays in the log"
    );
}

#[test]
fn saved_text_falls_back_to_the_full_path_without_a_file_name() {
    assert_eq!(
        saved_text(&PathBuf::from("/")),
        "Saved: /",
        "a path with no file name must still produce a readable chip"
    );
}

#[test]
fn toast_from_outcome_success_builds_a_saved_file_name_success_chip() {
    let chip = toast_from_outcome(
        &Ok(PathBuf::from("./.tmp/capture-test-toast/shot.png")),
        &overlay(OverlayPosition::BottomRight, true, true, 2000),
    )
    .expect("a successful capture with the overlay enabled produces a chip");
    assert_eq!(chip.text, "Saved: shot.png", "chip text is the file name");
    assert_eq!(chip.color, ToastColor::Success, "success is a success chip");
    assert_eq!(chip.image, None, "capture notifications are text chips");
    assert_eq!(chip.corner, Corner::BottomRight, "default corner");
    assert_eq!(
        chip.duration_ms, 2000,
        "hold comes from [overlay].duration_ms"
    );
    assert_eq!(
        chip.monitor,
        MonitorSel::All,
        "on_all_monitors covers every monitor"
    );
}

#[test]
fn toast_from_outcome_success_with_overlay_disabled_shows_nothing() {
    assert!(
        toast_from_outcome(
            &Ok(PathBuf::from("./.tmp/shot.png")),
            &overlay(OverlayPosition::BottomRight, false, true, 2000),
        )
        .is_none(),
        "overlay.enabled = false suppresses the success chip"
    );
}

#[test]
fn toast_from_outcome_error_is_a_danger_chip_with_the_verbatim_message() {
    let chip = toast_from_outcome(
        &Err(capture_failure()),
        &overlay(OverlayPosition::TopLeft, true, false, 2000),
    )
    .expect("a failed capture always produces a chip");
    assert_eq!(
        chip.text, "no active window",
        "spec §4.2 requires exactly 'no active window' in the toast"
    );
    assert_eq!(chip.color, ToastColor::Danger, "failures are danger chips");
}

/// Under the default `en` locale the catalog `toast.*` values are the original
/// English messages (spec §2), so the localized chip mapping must produce the
/// `Display` text byte-for-byte — for every variant, through `chip_text` and
/// through the chip the toast path actually shows.
#[test]
fn chip_text_matches_display_text_under_english_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let errors = vec![
        Error::NoActiveWindow,
        Error::Template {
            source: template::Error::UnknownToken {
                token: "%month".to_owned(),
                template: "%month.%ext".to_owned(),
            },
        },
        Error::Save {
            path: PathBuf::from("./.tmp/capture-test-chip/x.png"),
            source: image::ImageError::IoError(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "disk refused the write",
            )),
        },
        Error::Io {
            path: PathBuf::from("./.tmp/capture-test-chip/FullScreen"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "directory vanished"),
        },
        Error::TooManyCollisions {
            path: PathBuf::from("./.tmp/capture-test-chip"),
            limit: COLLISION_MAX_COUNTERS,
        },
        Error::MonitorCaptureFailed {
            reason: "Not supported".to_owned(),
            source: Some(Box::new(xcap::XCapError::NotSupported)),
        },
        Error::WindowTitleFailed {
            reason: "Not supported".to_owned(),
            source: Some(Box::new(xcap::XCapError::NotSupported)),
        },
        Error::WindowCaptureFailed {
            reason: "Not supported".to_owned(),
            source: Some(Box::new(xcap::XCapError::NotSupported)),
        },
        Error::WindowListFailed {
            reason: "Not supported".to_owned(),
            source: Some(Box::new(xcap::XCapError::NotSupported)),
        },
        Error::MonitorListFailed {
            reason: "Not supported".to_owned(),
            source: Some(Box::new(xcap::XCapError::NotSupported)),
        },
        Error::NoMonitorsFound,
    ];

    for err in errors {
        let display = err.to_string();
        assert_eq!(
            chip_text(&err),
            display,
            "chip mapping must equal the Display text byte-for-byte"
        );
        let chip = toast_from_outcome(
            &Err::<PathBuf, Error>(err),
            &overlay(OverlayPosition::BottomRight, true, false, 2000),
        )
        .expect("a failed capture always produces a chip");
        assert_eq!(
            chip.text, display,
            "the shown chip must equal the Display text byte-for-byte"
        );
    }
}

#[test]
fn toast_from_outcome_error_shows_even_with_overlay_disabled() {
    assert!(
        toast_from_outcome(
            &Err(capture_failure()),
            &overlay(OverlayPosition::BottomRight, false, false, 2000),
        )
        .is_some(),
        "spec §4.3: an error chip survives overlay.enabled = false"
    );
}

#[test]
fn toast_from_outcome_maps_each_position_to_the_matching_corner() {
    let expected = [
        (OverlayPosition::TopLeft, Corner::TopLeft),
        (OverlayPosition::TopRight, Corner::TopRight),
        (OverlayPosition::BottomLeft, Corner::BottomLeft),
        (OverlayPosition::BottomRight, Corner::BottomRight),
    ];
    for (position, corner) in expected {
        let chip = toast_from_outcome(
            &Err(capture_failure()),
            &overlay(position, true, true, 2000),
        )
        .expect("every position maps to a chip");
        assert_eq!(chip.corner, corner, "{position:?} must anchor {corner:?}");
    }
}

#[test]
fn toast_from_outcome_selects_all_monitors_or_the_first_one() {
    let all = toast_from_outcome(
        &Err(capture_failure()),
        &overlay(OverlayPosition::BottomRight, true, true, 2000),
    )
    .expect("chip for on_all_monitors = true");
    assert_eq!(all.monitor, MonitorSel::All, "one chip per monitor");

    let single = toast_from_outcome(
        &Err(capture_failure()),
        &overlay(OverlayPosition::BottomRight, true, false, 2000),
    )
    .expect("chip for on_all_monitors = false");
    assert_eq!(
        single.monitor,
        MonitorSel::Index(0),
        "the GUI puts the primary monitor first, so index 0 is it (spec §4.3)"
    );
}

#[test]
fn toast_duration_ms_carries_the_config_value_and_clamps_absurd_one() {
    assert_eq!(
        toast_duration_ms(&overlay(OverlayPosition::BottomRight, true, true, 1500)),
        1500,
        "[overlay].duration_ms is the chip hold"
    );
    assert_eq!(
        toast_duration_ms(&overlay(OverlayPosition::BottomRight, true, true, u64::MAX)),
        u32::MAX,
        "a u64 config value clamps to the u32 chip hold instead of wrapping"
    );
}

#[test]
fn toast_duration_ms_keeps_zero_as_sticky() {
    assert_eq!(
        toast_duration_ms(&overlay(OverlayPosition::BottomRight, true, true, 0)),
        0,
        "0 means sticky until dismissed (crates/toast rule)"
    );
}

// ---------- toast handle hand-off ----------

/// A real toast service with the host backend and NO monitors: `show` would be
/// a logged no-op there, so these tests exercise the slot itself (the same
/// latest-only mailbox semantics the `hotkeys` settings mailbox has).
fn started_service() -> ToastService {
    let host = OverlayHost::new();
    ToastService::spawn(Arc::new(host.handle()), Vec::new(), None)
}

#[test]
fn handoff_has_no_handle_before_the_service_is_attached() {
    assert!(
        handoff_handle(&new_handoff()).is_none(),
        "a capture started before the GUI exists carries no toast handle"
    );
}

#[test]
fn attach_handoff_exposes_a_usable_handle() {
    let handoff = new_handoff();
    let service = started_service();
    attach_handoff(&handoff, service.handle());

    let handle = handoff_handle(&handoff).expect("the attached handle is readable");
    assert!(
        handle
            .show(
                toast_from_outcome(
                    &Ok(PathBuf::from("./.tmp/shot.png")),
                    &overlay(OverlayPosition::BottomRight, true, true, 2000),
                )
                .expect("enabled success chip")
            )
            .is_ok(),
        "the handle must accept a chip from a capture thread (no monitors → logged no-op)"
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
    let path = run_capture(
        CaptureCommand {
            mode: CaptureMode::FullScreen,
            settings,
        },
        Local::now(),
        // No GUI on this path: the chip is skipped, the file is the result.
        None,
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
}

/// Spec §2: the chip path localizes while the `Display` path (logs and the
/// headless CLI) does not. Under `ru` both chips render Russian catalog
/// text, but `Error::to_string()` captured under the same locale keeps the
/// English contract. `ru` is restored BEFORE the assertions.
#[test]
fn chip_text_renders_in_russian_under_ru_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let no_window = chip_text(&Error::NoActiveWindow);
    let save = chip_text(&Error::Save {
        path: PathBuf::from("./.tmp/capture-test-locale/x.png"),
        source: image::ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "disk refused the write",
        )),
    });
    // Captured under ru on purpose: Display must NOT follow the UI locale.
    let display = Error::NoActiveWindow.to_string();

    rust_i18n::set_locale("en");

    assert_eq!(
        no_window, "нет активного окна",
        "the chip must render the Russian toast text under ru"
    );
    assert!(
        save.starts_with("не удалось сохранить"),
        "the Save chip must carry the Russian prefix, got {save}"
    );
    assert!(
        save.contains("disk refused the write"),
        "the source reason stays verbatim in the chip, got {save}"
    );
    assert_eq!(
        display, "no active window",
        "Display must stay English for logs/CLI even when captured under ru"
    );
}

/// Stage 4 (spec §2): under each new locale the chip path localizes to that
/// locale's catalog text while the `Display` path (logs and the headless
/// CLI) stays English. `en` is restored BEFORE each assertion.
#[test]
fn chip_text_renders_in_each_locale_and_display_stays_english() {
    let _guard = locale_guard();
    let expected: [(&str, &str); 4] = [
        ("zh-CN", "没有活动窗口"),
        ("es", "ninguna ventana activa"),
        ("ja", "アクティブなウィンドウなし"),
        ("ko", "활성 창 없음"),
    ];

    for (locale, chip) in expected {
        rust_i18n::set_locale(locale);
        let no_window = chip_text(&Error::NoActiveWindow);
        // Captured under the locale on purpose: Display must NOT follow it.
        let display = Error::NoActiveWindow.to_string();
        rust_i18n::set_locale("en");

        assert_eq!(
            no_window, chip,
            "the chip must render the {locale} toast text under {locale}"
        );
        assert_ne!(
            no_window, "no active window",
            "the chip must not stay English under {locale}, got {no_window}"
        );
        assert_eq!(
            display, "no active window",
            "Display must stay English for logs/CLI even when captured under {locale}"
        );
    }
}
