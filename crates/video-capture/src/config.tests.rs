//! Unit tests for `config`: defaults, clamping, support flags. Pure logic,
//! deterministic — no clock, no OS, no capture.

use super::*;

#[test]
fn defaults_captureconfig_matches_library_defaults() {
    let cfg = CaptureConfig::default();
    assert_eq!(cfg.codec, Codec::H264);
    assert_eq!(cfg.bitrate_kbps, 15_000);
    assert_eq!(cfg.resolution, Resolution::P1080);
    assert_eq!(cfg.scale_filter, ScaleFilter::Bilinear);
    assert_eq!(cfg.fps, 30);
    assert_eq!(cfg.replay_seconds(), 0);
    assert_eq!(cfg.audio, AudioMode::None);
}

#[test]
fn defaults_enum_choices_match_library_defaults() {
    assert_eq!(Codec::default(), Codec::H264);
    assert_eq!(Resolution::default(), Resolution::P1080);
    assert_eq!(ScaleFilter::default(), ScaleFilter::Bilinear);
    assert_eq!(AudioMode::default(), AudioMode::None);
}

#[test]
fn replay_seconds_in_range_is_kept() {
    let cfg = CaptureConfig {
        replay_seconds: 30,
        ..CaptureConfig::default()
    };
    assert_eq!(cfg.replay_seconds(), 30);
}

#[test]
fn replay_seconds_zero_stays_zero() {
    let cfg = CaptureConfig {
        replay_seconds: 0,
        ..CaptureConfig::default()
    };
    assert_eq!(cfg.replay_seconds(), 0);
}

#[test]
fn replay_seconds_one_over_limit_clamps_to_600() {
    let cfg = CaptureConfig {
        replay_seconds: 601,
        ..CaptureConfig::default()
    };
    assert_eq!(cfg.replay_seconds(), 600);
}

#[test]
fn replay_seconds_far_over_limit_clamps_to_600() {
    let cfg = CaptureConfig {
        replay_seconds: u32::MAX,
        ..CaptureConfig::default()
    };
    assert_eq!(cfg.replay_seconds(), 600);
}

#[test]
fn target_size_full_keeps_native_size() {
    assert_eq!(Resolution::Full.target_size((2560, 1440)), (2560, 1440));
}

#[test]
fn target_size_p720_on_large_desktop_is_1280x720() {
    assert_eq!(Resolution::P720.target_size((2560, 1440)), (1280, 720));
}

#[test]
fn target_size_p1080_on_exact_desktop_is_1920x1080() {
    assert_eq!(Resolution::P1080.target_size((1920, 1080)), (1920, 1080));
}

#[test]
fn target_size_smaller_desktop_keeps_native_size() {
    // The library cannot upscale, so a desktop under the target keeps native.
    assert_eq!(Resolution::P720.target_size((1024, 600)), (1024, 600));
    assert_eq!(Resolution::P1080.target_size((1280, 720)), (1280, 720));
}

#[test]
fn target_size_one_axis_smaller_keeps_native_size() {
    // Width fits but height does not: no scaling is possible, so native wins.
    assert_eq!(Resolution::P1080.target_size((1920, 700)), (1920, 700));
}

#[test]
fn audio_mode_none_is_supported() {
    assert!(AudioMode::None.is_supported());
}

#[test]
fn audio_mode_sources_are_not_supported() {
    assert!(!AudioMode::System.is_supported());
    assert!(!AudioMode::Mic.is_supported());
    assert!(!AudioMode::Both.is_supported());
}

#[test]
fn bitrate_bps_converts_kbps_to_bits_per_second() {
    assert_eq!(CaptureConfig::default().bitrate_bps(), 15_000_000);
}

#[test]
fn bitrate_bps_extreme_saturates_without_overflow() {
    let cfg = CaptureConfig {
        bitrate_kbps: u32::MAX,
        ..CaptureConfig::default()
    };
    assert_eq!(cfg.bitrate_bps(), u32::MAX);
}

#[test]
fn target_size_p720_keeps_the_native_aspect_ratio() {
    // A 16:10 desktop scaled to 720 rows keeps its aspect: width follows the
    // desktop, so glyph strokes are not squashed anisotropically.
    assert_eq!(Resolution::P720.target_size((1680, 1050)), (1152, 720));
}

#[test]
fn target_size_p1080_on_ultrawide_desktop_scales_both_axes_uniformly() {
    assert_eq!(Resolution::P1080.target_size((3440, 1440)), (2580, 1080));
}

#[test]
fn scale_filter_defaults_to_the_bilinear_kernel() {
    assert_eq!(ScaleFilter::default(), ScaleFilter::Bilinear);
    assert_eq!(ScaleFilter::Bilinear.label(), "Bilinear");
}

#[test]
fn resolution_height_160_on_16_9_native_is_284x160() {
    // Arbitrary heights are allowed: width follows the aspect exactly as the
    // presets do — floor(1920 * 160 / 1080) = 284.
    assert_eq!(
        Resolution::Height(160).target_size((1920, 1080)),
        (284, 160)
    );
    assert_eq!(
        Resolution::Height(160).valid_target_size((1920, 1080)),
        Some((284, 160))
    );
}

#[test]
fn resolution_height_720_matches_the_p720_preset() {
    assert_eq!(Resolution::P720, Resolution::Height(720));
    assert_eq!(
        Resolution::Height(720).target_size((2560, 1440)),
        (1280, 720)
    );
    assert_eq!(
        Resolution::Height(720).target_size((1920, 1080)),
        (1280, 720)
    );
}

#[test]
fn resolution_height_1080_matches_the_p1080_preset() {
    assert_eq!(Resolution::P1080, Resolution::Height(1080));
    assert_eq!(Resolution::default(), Resolution::Height(1080));
    assert_eq!(
        Resolution::Height(1080).target_size((1920, 1080)),
        (1920, 1080)
    );
}

#[test]
fn resolution_height_above_native_height_stays_native() {
    // No upscale: a requested height over the desktop's own height keeps native.
    assert_eq!(
        Resolution::Height(2000).target_size((1920, 1080)),
        (1920, 1080)
    );
    assert_eq!(
        Resolution::Height(2000).valid_target_size((1920, 1080)),
        Some((1920, 1080))
    );
}

#[test]
fn resolution_full_valid_target_is_native() {
    assert_eq!(
        Resolution::Full.valid_target_size((2560, 1440)),
        Some((2560, 1440))
    );
}

#[test]
fn resolution_height_zero_valid_target_is_none() {
    // A zero-height target is unusable (empty resize/flip bytes), so it is
    // invalid rather than a silently-clamped 1-row frame.
    assert_eq!(Resolution::Height(0).valid_target_size((1920, 1080)), None);
    assert_eq!(Resolution::Height(0).valid_target_size((0, 0)), None);
}

#[test]
fn resolution_height_zero_target_size_falls_back_to_native() {
    // `target_size` never panics: without a valid target it reports native,
    // and the `Recorder` constructors refuse such a configuration up front
    // (see `CaptureConfig::invalid_reason`).
    assert_eq!(
        Resolution::Height(0).target_size((1920, 1080)),
        (1920, 1080)
    );
}

#[test]
fn resolution_height_on_21_9_native_derives_width_from_aspect() {
    // 2560x1080 is 21:9: floor(2560 * 720 / 1080) = 1706, floor(2560 * 160 /
    // 1080) = 379 — the width follows the desktop, not a fixed 16:9 box.
    assert_eq!(
        Resolution::Height(720).target_size((2560, 1080)),
        (1706, 720)
    );
    assert_eq!(
        Resolution::Height(160).target_size((2560, 1080)),
        (379, 160)
    );
}

#[test]
fn resolution_height_tiny_width_floor_clamps_to_one() {
    // floor(1 * 720 / 1080) = 0, clamped to 1: the width never becomes zero.
    assert_eq!(Resolution::Height(720).target_size((1, 1080)), (1, 720));
}

#[test]
fn resolution_label_height_reads_as_number_p() {
    assert_eq!(Resolution::Height(160).label(), "160p");
    assert_eq!(Resolution::P720.label(), "720p");
    assert_eq!(Resolution::P1080.label(), "1080p");
}

#[test]
fn resolution_label_full_reads_as_full() {
    assert_eq!(Resolution::Full.label(), "Full");
}

#[test]
fn scale_filter_separates_own_kernels_from_image_kernels() {
    assert!(!ScaleFilter::Nearest.is_image_kernel());
    assert!(!ScaleFilter::Bilinear.is_image_kernel());
    for filter in [
        ScaleFilter::CatmullRom,
        ScaleFilter::Gaussian,
        ScaleFilter::Lanczos3,
    ] {
        assert!(
            filter.is_image_kernel(),
            "{filter:?} must run through image"
        );
        assert!(!filter.label().is_empty());
    }
}

#[test]
fn captureconfig_default_fps_is_30() {
    assert_eq!(CaptureConfig::default().fps, 30);
}

#[test]
fn frame_interval_thirty_fps_is_floor_of_one_over_30_seconds() {
    let cfg = CaptureConfig {
        fps: 30,
        ..CaptureConfig::default()
    };

    assert_eq!(cfg.frame_interval(), Duration::from_nanos(33_333_333));
}

#[test]
fn frame_interval_sixty_fps_is_floor_of_one_over_60_seconds() {
    let cfg = CaptureConfig {
        fps: 60,
        ..CaptureConfig::default()
    };

    assert_eq!(cfg.frame_interval(), Duration::from_nanos(16_666_666));
}

#[test]
fn frame_interval_one_fps_is_exactly_one_second() {
    let cfg = CaptureConfig {
        fps: 1,
        ..CaptureConfig::default()
    };

    assert_eq!(cfg.frame_interval(), Duration::from_secs(1));
}

#[test]
fn frame_interval_zero_fps_clamps_instead_of_dividing_by_zero() {
    // `fps == 0` is invalid and rejected before capture, but the pure helper
    // must not panic on it.
    let cfg = CaptureConfig {
        fps: 0,
        ..CaptureConfig::default()
    };

    assert_eq!(cfg.frame_interval(), Duration::from_secs(1));
}

#[test]
fn ring_frames_for_u8_fps_multiplies_seconds_and_fps() {
    // The engine feeds `u32::from(config.fps)`; the ring math stays seconds × fps.
    assert_eq!(crate::ring::frames_for(5, u32::from(60u8)), 300);
    assert_eq!(crate::ring::frames_for(5, u32::from(30u8)), 150);
    assert_eq!(crate::ring::frames_for(0, u32::from(60u8)), 0);
}

#[test]
fn invalid_reason_zero_fps_reports_a_message() {
    let cfg = CaptureConfig {
        fps: 0,
        ..CaptureConfig::default()
    };

    assert_eq!(
        cfg.invalid_reason(),
        Some("частота кадров 0 недопустима: нужен fps от 1")
    );
}

#[test]
fn invalid_reason_zero_height_reports_a_message() {
    let cfg = CaptureConfig {
        resolution: Resolution::Height(0),
        ..CaptureConfig::default()
    };

    assert_eq!(
        cfg.invalid_reason(),
        Some("высота разрешения 0 недопустима: нужна высота от 1 пикселя")
    );
}

#[test]
fn invalid_reason_default_config_is_valid() {
    assert_eq!(CaptureConfig::default().invalid_reason(), None);
}

#[test]
fn invalid_reason_custom_height_and_fps_are_valid() {
    let cfg = CaptureConfig {
        fps: 60,
        resolution: Resolution::Height(160),
        ..CaptureConfig::default()
    };

    assert_eq!(cfg.invalid_reason(), None);
    assert_eq!(
        CaptureConfig {
            resolution: Resolution::Full,
            ..CaptureConfig::default()
        }
        .invalid_reason(),
        None
    );
}
