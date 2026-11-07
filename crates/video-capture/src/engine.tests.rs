//! Unit tests for `engine`'s platform-independent logic: BGRA resize, BGRA row
//! flipping, frame pacing, ring sizing, output-path validation, the ack-pending
//! decision, and the Linux facade contract. No real capture, no filesystem
//! creation — timestamps and the ack clock are injected.

use super::*;

/// A 2x1 frame: one dark pixel and one bright pixel, opaque alpha.
fn two_pixel_frame() -> Vec<u8> {
    vec![
        10, 20, 30, 255, // pixel 0
        200, 40, 60, 255, // pixel 1
    ]
}

#[test]
fn resize_bgra_nearest_downscale_returns_destination_byte_count() {
    let bytes = vec![0u8; frame_bytes(4, 2)];
    let resized = resize_bgra(&bytes, 4, 2, 2, 1, ScaleFilter::Nearest);

    assert_eq!(resized.len(), frame_bytes(2, 1));
}

#[test]
fn resize_bgra_nearest_downscale_to_single_pixel_keeps_a_source_pixel() {
    // Nearest sampling picks one of the source pixels; it must never blend the
    // two into a color that was never present, and alpha must stay opaque.
    let resized = resize_bgra(&two_pixel_frame(), 2, 1, 1, 1, ScaleFilter::Nearest);

    assert_eq!(resized.len(), frame_bytes(1, 1));
    assert!(
        resized == [10, 20, 30, 255].to_vec() || resized == [200, 40, 60, 255].to_vec(),
        "nearest resize produced a blended pixel: {resized:?}"
    );
}

#[test]
fn resize_bgra_nearest_upscale_repeats_the_source_pixel() {
    let source = vec![12, 34, 56, 255];
    let resized = resize_bgra(&source, 1, 1, 3, 2, ScaleFilter::Nearest);

    assert_eq!(resized.len(), frame_bytes(3, 2));
    assert_eq!(
        resized,
        [12, 34, 56, 255].repeat(6),
        "nearest upscale did not repeat the source pixel"
    );
}

#[test]
fn resize_bgra_bilinear_downscale_to_single_pixel_blends_the_source() {
    // Bilinear is the default precisely because it keeps thin strokes: a
    // downscaled pixel is the midpoint of the source pixels it covers.
    let resized = resize_bgra(&two_pixel_frame(), 2, 1, 1, 1, ScaleFilter::Bilinear);

    assert_eq!(resized, [105, 30, 45, 255].to_vec());
}

#[test]
fn resize_bgra_bilinear_upscale_interpolates_and_clamps_edges() {
    // The middle destination pixel lies between the two source pixels; the edge
    // pixels clamp to the first and last source pixel instead of extrapolating.
    let resized = resize_bgra(&two_pixel_frame(), 2, 1, 3, 1, ScaleFilter::Bilinear);

    assert_eq!(
        resized,
        [
            10, 20, 30, 255, // clamped to source pixel 0
            105, 30, 45, 255, // midpoint
            200, 40, 60, 255, // clamped to source pixel 1
        ]
    );
}

#[test]
fn resize_bgra_image_filter_keeps_a_solid_color() {
    // The `image`-backed filters must not introduce colors a solid source never
    // had, and alpha must stay opaque.
    let source = [12u8, 34, 56, 255].repeat(frame_bytes(4, 2) / 4);

    for filter in [
        ScaleFilter::CatmullRom,
        ScaleFilter::Gaussian,
        ScaleFilter::Lanczos3,
    ] {
        let resized = resize_bgra(&source, 4, 2, 2, 1, filter);
        assert_eq!(resized.len(), frame_bytes(2, 1));
        assert_eq!(
            resized,
            [12, 34, 56, 255].repeat(2),
            "{filter:?} changed a solid color"
        );
    }
}

#[test]
fn resize_bgra_same_size_returns_identical_bytes() {
    let bytes = two_pixel_frame();

    for filter in [
        ScaleFilter::Nearest,
        ScaleFilter::Bilinear,
        ScaleFilter::Lanczos3,
    ] {
        assert_eq!(resize_bgra(&bytes, 2, 1, 2, 1, filter), bytes);
    }
}

#[test]
fn resize_bgra_short_input_returns_empty() {
    let resized = resize_bgra(&two_pixel_frame(), 4, 2, 2, 1, ScaleFilter::Nearest);

    assert!(resized.is_empty(), "oversized source must not be resized");
}

#[test]
fn resize_bgra_zero_destination_returns_empty() {
    let bytes = vec![0u8; frame_bytes(4, 2)];

    assert!(resize_bgra(&bytes, 4, 2, 0, 1, ScaleFilter::Nearest).is_empty());
    assert!(resize_bgra(&bytes, 4, 2, 1, 0, ScaleFilter::Bilinear).is_empty());
}

#[test]
fn resize_bgra_zero_source_returns_empty() {
    assert!(resize_bgra(&[], 0, 0, 2, 2, ScaleFilter::Nearest).is_empty());
}

#[test]
fn flip_rows_bgra_two_by_two_swaps_row_order() {
    let bytes = [
        10, 20, 30, 255, 11, 21, 31, 255, // row 0
        40, 50, 60, 255, 41, 51, 61, 255, // row 1
    ];

    let flipped = flip_rows_bgra(&bytes, 2, 2);

    assert_eq!(flipped.len(), frame_bytes(2, 2));
    assert_eq!(
        &flipped[..8],
        &bytes[8..16],
        "output row 0 must be input row 1"
    );
    assert_eq!(
        &flipped[8..],
        &bytes[..8],
        "output row 1 must be input row 0"
    );
}

#[test]
fn flip_rows_bgra_single_row_is_identity() {
    // A 2x1 frame has nothing to reorder: the bytes come back unchanged.
    let bytes = two_pixel_frame();

    assert_eq!(flip_rows_bgra(&bytes, 2, 1), bytes);
}

#[test]
fn flip_rows_bgra_twice_is_identity() {
    // The feed flips rows once; flipping again must restore the original
    // buffer, so pixels and their BGRA channel order are never rewritten.
    let bytes = [
        10, 20, 30, 255, 11, 21, 31, 255, 40, 50, 60, 255, 41, 51, 61, 255, 70, 80, 90, 255, 71,
        81, 91, 255,
    ];

    let once = flip_rows_bgra(&bytes, 2, 3);
    assert_eq!(once.len(), frame_bytes(2, 3));
    assert_eq!(flip_rows_bgra(&once, 2, 3), bytes);
}

#[test]
fn flip_rows_bgra_wrong_length_or_zero_size_returns_empty() {
    // Too short for 2x2, and degenerate dimensions must not produce bytes.
    assert!(flip_rows_bgra(&two_pixel_frame(), 2, 2).is_empty());
    assert!(flip_rows_bgra(&vec![0u8; frame_bytes(2, 2) + 4], 2, 2).is_empty());
    assert!(flip_rows_bgra(&two_pixel_frame(), 2, 0).is_empty());
    assert!(flip_rows_bgra(&[], 0, 0).is_empty());
}

#[test]
fn should_send_first_frame_without_baseline_is_accepted() {
    assert!(should_send(1_000, None, 333_333));
}

#[test]
fn should_send_equal_timestamp_is_dropped() {
    assert!(!should_send(1_000, Some(1_000), 333_333));
}

#[test]
fn should_send_delta_below_interval_is_dropped() {
    assert!(!should_send(1_000 + 333_332, Some(1_000), 333_333));
}

#[test]
fn should_send_delta_at_interval_is_accepted() {
    assert!(should_send(1_000 + 333_333, Some(1_000), 333_333));
}

#[test]
fn should_send_timestamp_before_baseline_is_dropped() {
    // Clock jitter backwards must not produce a re-timed frame.
    assert!(!should_send(900, Some(1_000), 333_333));
}

#[test]
fn interval_ticks_thirty_fps_is_333333_ticks() {
    let config = CaptureConfig {
        fps: 30,
        ..CaptureConfig::default()
    };

    assert_eq!(interval_ticks(&config), 333_333);
}

#[test]
fn interval_ticks_sixty_fps_is_166666_ticks() {
    let config = CaptureConfig {
        fps: 60,
        ..CaptureConfig::default()
    };

    assert_eq!(interval_ticks(&config), 166_666);
}

#[test]
fn interval_ticks_one_fps_is_a_second_in_ticks() {
    let config = CaptureConfig {
        fps: 1,
        ..CaptureConfig::default()
    };

    assert_eq!(interval_ticks(&config), 10_000_000);
}

#[test]
fn ring_capacity_matches_replay_seconds_times_fps() {
    let config = CaptureConfig {
        replay_seconds: 5,
        fps: 60,
        ..CaptureConfig::default()
    };

    assert_eq!(ring_capacity(&config), 300);
}

#[test]
fn ring_capacity_of_default_replay_zero_stores_nothing() {
    assert_eq!(ring_capacity(&CaptureConfig::default()), 0);
}

#[test]
fn session_should_release_zero_capacity_idle_requests_release() {
    // A disabled replay ring and no recording in progress: nothing will ever
    // be buffered again, so the capture session must be released.
    assert!(session_should_release(0, false));
}

#[test]
fn session_should_release_zero_capacity_recording_keeps_session_open() {
    // The recording still needs frames from the session even though the ring
    // stores nothing; the release happens only after the recording stops.
    assert!(!session_should_release(0, true));
}

#[test]
fn session_should_release_nonzero_capacity_idle_keeps_session_open() {
    // A ring that still wants frames needs the session to keep producing them.
    assert!(!session_should_release(1, false));
    assert!(!session_should_release(
        usize::try_from(30 * u32::from(CaptureConfig::default().fps)).unwrap(),
        false,
    ));
    assert!(!session_should_release(usize::MAX, false));
}

#[test]
fn session_should_release_nonzero_capacity_recording_keeps_session_open() {
    // A live recording on a ring that stores frames: the session must stay
    // open for both reasons at once.
    assert!(!session_should_release(1, true));
    assert!(!session_should_release(
        usize::try_from(30 * u32::from(CaptureConfig::default().fps)).unwrap(),
        true,
    ));
}

#[test]
fn config_dump_contains_codec_bitrate_resolution_scale_filter_fps_replay_audio() {
    let config = CaptureConfig {
        codec: crate::config::Codec::Hevc,
        bitrate_kbps: 8_000,
        resolution: crate::config::Resolution::P720,
        scale_filter: crate::config::ScaleFilter::Lanczos3,
        fps: 60,
        replay_seconds: 5,
        audio: crate::config::AudioMode::None,
    };

    let dump = config_dump(&config);

    for part in [
        "codec=Hevc",
        "bitrate_kbps=8000",
        "resolution=Height(720)",
        "scale_filter=Lanczos3",
        "fps=60",
        "replay_seconds=5",
        "audio=None",
    ] {
        assert!(dump.contains(part), "dump is missing {part}: {dump}");
    }

    // Rate control is CBR only, so the dump names no mode at all.
    assert!(
        !dump.contains("bitrate_mode"),
        "a deleted option must not appear in the dump: {dump}"
    );

    // Supported choices carry no marker, and the dump stays one line.
    assert!(
        !dump.contains("unsupported"),
        "supported config got a marker: {dump}"
    );
    assert_eq!(
        dump.lines().count(),
        1,
        "dump must be a single line: {dump}"
    );
}

#[test]
fn config_dump_marks_unsupported_audio_only() {
    let config = CaptureConfig {
        audio: crate::config::AudioMode::System,
        ..CaptureConfig::default()
    };

    let dump = config_dump(&config);

    assert!(
        dump.contains("audio=System (unsupported"),
        "unsupported audio source is not marked: {dump}"
    );
    // The base parameters are still reported, so the log shows what was asked.
    assert!(
        dump.contains("bitrate_kbps=15000"),
        "base parameters lost: {dump}"
    );
    assert_eq!(
        dump.lines().count(),
        1,
        "dump must be a single line: {dump}"
    );
}

#[test]
fn validate_output_path_empty_is_invalid_path() {
    let error = validate_output_path(Path::new("")).expect_err("empty path must be rejected");

    assert!(matches!(error, CaptureError::InvalidPath(_)));
}

#[test]
fn validate_output_path_missing_directory_is_invalid_path() {
    let error = validate_output_path(Path::new("/nonexistent-dir-for-tests/video.mp4"))
        .expect_err("missing parent directory must be rejected");

    assert!(matches!(error, CaptureError::InvalidPath(_)));
}

#[test]
fn validate_output_path_relative_name_is_accepted() {
    // A bare filename means the current directory, which always exists.
    assert!(validate_output_path(Path::new("video.mp4")).is_ok());
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_new_keeps_config_without_capture() {
    let recorder =
        Recorder::new(CaptureConfig::default()).expect("construction is platform-neutral");

    assert_eq!(recorder.config(), CaptureConfig::default());
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_start_returns_unsupported_platform() {
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    let error = recorder
        .start(Path::new("video.mp4"))
        .expect_err("Linux has no capture");
    assert!(matches!(error, CaptureError::UnsupportedPlatform));
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_start_returns_unsupported_platform_for_empty_path() {
    // The platform check comes first, so path validation is never consulted.
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    let error = recorder
        .start(Path::new(""))
        .expect_err("empty path must be rejected");
    assert!(matches!(error, CaptureError::UnsupportedPlatform));
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_stop_returns_unsupported_platform() {
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    let error = recorder.stop().expect_err("Linux has no capture");
    assert!(matches!(error, CaptureError::UnsupportedPlatform));
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_shutdown_returns_unsupported_platform() {
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    let error = recorder
        .shutdown()
        .expect_err("Linux has no capture thread");
    assert!(matches!(error, CaptureError::UnsupportedPlatform));
}

#[cfg(not(windows))]
#[test]
fn facade_enable_replay_on_linux_returns_unsupported_platform() {
    // The Linux facade has no capture thread, so a replay request cannot reach
    // a ring; it reports the same platform failure as `start`/`stop`.
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    let error = recorder
        .enable_replay(5)
        .expect_err("Linux has no replay ring");
    assert!(matches!(error, CaptureError::UnsupportedPlatform));
}

#[cfg(not(windows))]
#[test]
fn facade_disable_replay_on_linux_returns_unsupported_platform() {
    let mut recorder = Recorder::new(CaptureConfig {
        replay_seconds: 5,
        ..CaptureConfig::default()
    })
    .expect("construction succeeds");

    let error = recorder
        .disable_replay()
        .expect_err("Linux has no replay ring");
    assert!(matches!(error, CaptureError::UnsupportedPlatform));
}

#[cfg(not(windows))]
#[test]
fn facade_replay_enabled_follows_config_then_requests() {
    // The state is local to the facade: seeded from the startup config and
    // updated by every request, so it never roundtrips to a capture thread
    // (and on Linux there is no thread to roundtrip to).
    let enabled = Recorder::new(CaptureConfig {
        replay_seconds: 5,
        ..CaptureConfig::default()
    })
    .expect("construction succeeds");
    assert!(
        enabled.replay_enabled(),
        "replay_seconds > 0 must report enabled"
    );

    let mut disabled = Recorder::new(CaptureConfig::default()).expect("construction succeeds");
    assert!(
        !disabled.replay_enabled(),
        "replay_seconds == 0 must report disabled"
    );

    // A failed Linux request still records what was asked for.
    assert!(disabled.enable_replay(3).is_err());
    assert!(
        disabled.replay_enabled(),
        "enable request must update the reported state"
    );

    assert!(disabled.disable_replay().is_err());
    assert!(
        !disabled.replay_enabled(),
        "disable request must update the reported state"
    );

    assert!(disabled.enable_replay(0).is_err());
    assert!(
        !disabled.replay_enabled(),
        "enable(0) must read as a disabled buffer"
    );
}

#[test]
fn event_is_capture_release_matches_only_the_release_event() {
    // The release event is neither command's ack: it must never complete a
    // pending `Start` or `Stop`, and no other event reports the session gone.
    assert!(event_is_capture_release(&EngineEvent::CaptureReleased));

    assert!(
        !event_is_capture_release(&EngineEvent::RecordingStarted {
            path: PathBuf::from("video.mp4")
        }),
        "the start ack is not a release"
    );
    assert!(
        !event_is_capture_release(&EngineEvent::RecordingStopped),
        "the stop ack is not a release"
    );
    assert!(
        !event_is_capture_release(&EngineEvent::CommandFailed {
            error: CaptureError::NotConfigured,
        }),
        "a command failure is not a release"
    );
    assert!(
        !event_is_capture_release(&EngineEvent::FrameFailed {
            error: CaptureError::UnsupportedPlatform,
        }),
        "a dropped frame is not a release"
    );
}

#[test]
fn ack_outcome_idle_reports_no_outcome() {
    // Nothing queued (the Linux facade's steady state) — nothing to report.
    let mut pending = AckState::Idle;

    assert!(ack_outcome("start", &mut pending, Instant::now()).is_none());
    assert!(matches!(pending, AckState::Idle));
}

#[test]
fn ack_outcome_pending_before_deadline_reports_none_and_keeps_the_deadline() {
    // A queued command stays pending: the deadline must survive so a later poll
    // can still report the ack or the timeout.
    let now = Instant::now();
    let deadline = now + std::time::Duration::from_secs(5);
    let mut pending = AckState::Pending { deadline };

    assert!(ack_outcome("stop", &mut pending, now).is_none());

    match &pending {
        AckState::Pending { deadline: kept } => {
            assert_eq!(*kept, deadline, "deadline must be kept");
        }
        other => panic!("expected a pending deadline, got {other:?}"),
    }
}

#[test]
fn ack_outcome_pending_at_deadline_reports_timeout_once() {
    // The boundary is inclusive: `now == deadline` is already expired, and the
    // timeout consumes the state so a second poll reports nothing again.
    let deadline = Instant::now();
    let mut pending = AckState::Pending { deadline };

    let outcome =
        ack_outcome("start", &mut pending, deadline).expect("an elapsed deadline must be reported");
    let error = outcome.expect_err("an elapsed deadline is a failure");

    assert!(matches!(error, CaptureError::Encoder(_)));
    assert!(
        error
            .to_string()
            .contains("did not acknowledge the start request"),
        "timeout must name the command: {error}"
    );
    assert!(
        matches!(pending, AckState::Idle),
        "the timeout must consume the pending state"
    );
    assert!(
        ack_outcome("start", &mut pending, deadline).is_none(),
        "a timeout is reported once"
    );
}

#[test]
fn ack_outcome_acked_reports_ok_once() {
    let mut pending = AckState::Acked;

    assert!(matches!(
        ack_outcome("stop", &mut pending, Instant::now()),
        Some(Ok(()))
    ));
    assert!(matches!(pending, AckState::Idle));
    assert!(
        ack_outcome("stop", &mut pending, Instant::now()).is_none(),
        "an ack is reported exactly once"
    );
}

#[test]
fn ack_outcome_failed_reports_the_typed_error_once() {
    let mut pending = AckState::Failed(CaptureError::NotConfigured);

    let outcome = ack_outcome("start", &mut pending, Instant::now())
        .expect("a failure event must be reported");
    assert!(matches!(outcome, Err(CaptureError::NotConfigured)));
    assert!(matches!(pending, AckState::Idle));
    assert!(
        ack_outcome("start", &mut pending, Instant::now()).is_none(),
        "a failure is reported exactly once"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_start_on_linux_reports_unsupported_platform() {
    // `start` took `impl Into<PathBuf>` and stopped waiting for the ack, but the
    // Linux contract is unchanged: nothing is queued, so no ack is pending.
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    let error = recorder
        .start("video.mp4")
        .expect_err("Linux has no capture");
    assert!(matches!(error, CaptureError::UnsupportedPlatform));

    assert!(
        recorder.poll_recording_ack().is_none(),
        "a failed start must not mark a pending ack"
    );
    assert!(
        recorder.poll_stop_ack().is_none(),
        "a failed start must not mark a pending stop"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_recording_on_linux_reports_unsupported_platform() {
    // The Windows boot helper queues `EngineCommand::Start` before the capture
    // thread exists; on Linux there is no thread to boot, so it reports the
    // same platform failure as the other stubs and validates nothing.
    let outcome = Recorder::recording(CaptureConfig::default(), "");

    assert!(
        matches!(outcome, Err(CaptureError::UnsupportedPlatform)),
        "Linux has no capture thread to boot"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_poll_acks_on_linux_are_not_pending() {
    // Both polls are non-blocking (`None`, never a wall-clock wait) and the
    // Linux facade never queues, so they report `None` before and after failed
    // capture calls alike.
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    assert!(recorder.poll_recording_ack().is_none());
    assert!(recorder.poll_stop_ack().is_none());

    assert!(recorder.start("video.mp4").is_err());
    assert!(recorder.stop().is_err());

    assert!(recorder.poll_recording_ack().is_none());
    assert!(recorder.poll_stop_ack().is_none());
}

#[cfg(not(windows))]
#[test]
fn facade_start_after_release_reports_not_configured() {
    // Once the capture thread reported `CaptureReleased`, the session is gone:
    // `start` is refused with the same error `shutdown` reports, *before* path
    // validation (an otherwise-valid path is still refused). `Recorder` is not
    // `Debug`, so the outcome is matched rather than unwrapped.
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");
    recorder.session_released = true;

    assert!(
        matches!(
            recorder.start("video.mp4"),
            Err(CaptureError::NotConfigured)
        ),
        "a released session must refuse start"
    );
    assert!(
        matches!(
            recorder.start(Path::new("")),
            Err(CaptureError::NotConfigured)
        ),
        "a released session must refuse start before path validation"
    );
    assert!(
        recorder.poll_recording_ack().is_none(),
        "a refused start must not mark a pending ack"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_stop_after_release_reports_not_configured() {
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");
    recorder.session_released = true;

    assert!(
        matches!(recorder.stop(), Err(CaptureError::NotConfigured)),
        "a released session must refuse stop"
    );
    assert!(
        recorder.poll_stop_ack().is_none(),
        "a refused stop must not mark a pending ack"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_enable_replay_after_release_reports_not_configured() {
    // A released session has no ring to address, so the request is refused
    // before anything else, for both enable directions.
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");
    recorder.session_released = true;

    assert!(
        matches!(recorder.enable_replay(5), Err(CaptureError::NotConfigured)),
        "a released session must refuse an enable request"
    );
    assert!(
        matches!(recorder.enable_replay(0), Err(CaptureError::NotConfigured)),
        "a released session must refuse a disable-shaped request"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_disable_replay_after_release_reports_not_configured() {
    let mut recorder = Recorder::new(CaptureConfig {
        replay_seconds: 5,
        ..CaptureConfig::default()
    })
    .expect("construction succeeds");
    recorder.session_released = true;

    assert!(
        matches!(recorder.disable_replay(), Err(CaptureError::NotConfigured)),
        "a released session must refuse a disable request"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_replay_enabled_after_release_still_reports_requested_state() {
    // The release guard refuses commands but never touches the replay state:
    // `replay_enabled` keeps answering from the facade's local state, and a
    // refused request does not change it.
    let mut recorder = Recorder::new(CaptureConfig {
        replay_seconds: 5,
        ..CaptureConfig::default()
    })
    .expect("construction succeeds");
    assert!(recorder.replay_enabled());

    recorder.session_released = true;
    assert!(
        recorder.replay_enabled(),
        "release must not change the reported replay state"
    );

    assert!(matches!(
        recorder.disable_replay(),
        Err(CaptureError::NotConfigured)
    ));
    assert!(
        recorder.replay_enabled(),
        "a refused disable must not change the reported replay state"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_poll_session_release_without_capture_reports_not_released() {
    // The Linux stub starts no capture, so no release event can arrive and the
    // flag stays false.
    let recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");

    assert!(
        !recorder.poll_session_release(),
        "a stub recorder can never report a released session"
    );
}

#[cfg(not(windows))]
#[test]
fn facade_poll_session_release_reports_recorded_release() {
    // The poll reads the recorded flag: a release marked earlier must be
    // visible without any command being queued.
    let mut recorder = Recorder::new(CaptureConfig::default()).expect("construction succeeds");
    recorder.session_released = true;

    assert!(
        recorder.poll_session_release(),
        "a recorded release must be readable without an ack poll"
    );
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_new_rejects_zero_fps_with_not_configured() {
    // Configuration validation is platform-independent: a zero fps has no
    // pacing interval, so the constructor refuses it before any platform check.
    let outcome = Recorder::new(CaptureConfig {
        fps: 0,
        ..CaptureConfig::default()
    });

    assert!(
        matches!(outcome, Err(CaptureError::NotConfigured)),
        "fps 0 must be refused with the typed error"
    );
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_recording_rejects_zero_height_with_not_configured() {
    // `Height(0)` has no usable target size, so the boot helper refuses it too.
    let outcome = Recorder::recording(
        CaptureConfig {
            resolution: crate::config::Resolution::Height(0),
            ..CaptureConfig::default()
        },
        "video.mp4",
    );

    assert!(
        matches!(outcome, Err(CaptureError::NotConfigured)),
        "Height(0) must be refused with the typed error"
    );
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_with_debug_dump_rejects_zero_fps_with_not_configured() {
    // The dump variant is a constructor as well: an unusable config is a
    // configuration failure, not a platform failure.
    let outcome = Recorder::with_debug_dump(
        CaptureConfig {
            fps: 0,
            ..CaptureConfig::default()
        },
        Some(std::path::PathBuf::from(".tmp/dumps")),
    );

    assert!(
        matches!(outcome, Err(CaptureError::NotConfigured)),
        "fps 0 must be refused with the typed error"
    );
}

#[cfg(not(windows))]
#[test]
fn recorder_linux_new_accepts_custom_height_and_positive_fps() {
    // A valid non-preset configuration stays accepted: validation rejects only
    // the two unusable values, not arbitrary heights.
    let recorder = Recorder::new(CaptureConfig {
        fps: 60,
        resolution: crate::config::Resolution::Height(160),
        ..CaptureConfig::default()
    })
    .expect("a valid custom configuration is accepted");

    assert_eq!(recorder.config().fps, 60);
}
