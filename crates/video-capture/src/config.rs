//! Capture configuration: pure, platform-independent encoding parameters.
//!
//! The values here mirror what the underlying `windows-capture` library can
//! actually honor: codec, target bitrate (CBR — the library exposes a bitrate
//! value, not rate-control modes), resolution, scale filter, frame rate, replay
//! window. Audio is the one *marker* left: [`AudioMode::is_supported`] says
//! whether a capture can record the source, so callers reject an unsupported
//! source before capture instead of at the encoder boundary.
//! [`CaptureConfig::invalid_reason`] is the tight pre-flight check for the
//! values no capture can use (`fps == 0`, `Resolution::Height(0)`).

use std::time::Duration;

/// Codec profile. The library has no separate H.265 value: [`Codec::Hevc`]
/// *is* h.265 (HEVC is the standard name), so the enum carries the two names
/// the library exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Codec {
    /// H.264 / AVC — the library default.
    #[default]
    H264,
    /// HEVC — the library's name for h.265; there is no separate H265 value.
    Hevc,
}

/// Output resolution choice: an arbitrary height in rows, or the desktop's
/// native size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// Fixed *height* in pixels; the width follows the desktop's aspect ratio.
    /// Any height is selectable (e.g. `Height(160)`); `Height(0)` is invalid —
    /// it has no usable target, so [`Resolution::target_size`] reports native.
    Height(u16),
    /// Keep the desktop's native size as-is.
    Full,
}

impl Resolution {
    /// 720 rows tall, width proportional to the desktop's aspect ratio.
    pub const P720: Self = Self::Height(720);

    /// 1080 rows tall, width proportional to the desktop's aspect ratio — the
    /// default.
    pub const P1080: Self = Self::Height(1080);

    /// Size to request for a given native desktop size.
    ///
    /// The choice fixes the *height*; the width follows the desktop's aspect
    /// ratio (`native_width * height / native_height`, floor, at least 1), so
    /// scaling is uniform on both axes and glyph strokes are not anisotropically
    /// squashed. A desktop shorter than the requested height (or degenerate)
    /// keeps its native size: the engine scales down, never up.
    ///
    /// `Height(0)` has no usable target, so it reports the native size instead
    /// of a 1-row frame: nothing here panics, and the engine rejects a zero
    /// height before capture starts.
    pub fn target_size(&self, native: (u32, u32)) -> (u32, u32) {
        self.valid_target_size(native).unwrap_or(native)
    }

    /// Target size, or `None` when the choice cannot produce a usable target:
    /// [`Resolution::Height(0)`] yields a zero-height frame, which the resize
    /// and flip paths reject (empty bytes) and a 0x0 encoder texture cannot
    /// exist. A degenerate native size keeps native, as with `Full`.
    fn valid_target_size(&self, native: (u32, u32)) -> Option<(u32, u32)> {
        match self {
            Self::Full => Some(native),
            Self::Height(0) => None,
            Self::Height(height) => Some(scaled_height(u32::from(*height), native)),
        }
    }

    /// Short display label for UI/status output: `"{height}p"` for a fixed
    /// height (`Height(160)` → `"160p"`), `"Full"` unchanged.
    pub fn label(&self) -> String {
        match self {
            Self::Height(height) => format!("{height}p"),
            Self::Full => "Full".to_string(),
        }
    }
}

impl Default for Resolution {
    fn default() -> Self {
        Self::P1080
    }
}

/// Requested height with the native aspect ratio applied to the width.
fn scaled_height(height: u32, native: (u32, u32)) -> (u32, u32) {
    if native.0 == 0 || native.1 == 0 || native.1 < height {
        return native;
    }

    let width = (u64::from(native.0) * u64::from(height) / u64::from(native.1)) as u32;

    (width.max(1), height)
}

/// Scaling filter for the per-frame CPU resize.
///
/// Only used when a captured frame's size differs from the encoder target: on
/// [`Resolution::Full`] frames go through the GPU surface and nothing is
/// scaled. `Nearest` and `Bilinear` are this crate's own kernels (nearest pick,
/// separable bilinear — cheap enough for a live loop); `CatmullRom`, `Gaussian`,
/// and `Lanczos3` run through `image::imageops::resize`, which is measurably
/// slower per frame (measured costs: README).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScaleFilter {
    /// Pick the nearest source pixel. Cheapest, but thin glyph strokes can be
    /// dropped when downscaling.
    Nearest,
    /// Separable bilinear interpolation — the default: strokes survive, slight
    /// blur.
    #[default]
    Bilinear,
    /// Catmull-Rom (cubic) — sharper than bilinear, via `image`.
    CatmullRom,
    /// Gaussian — smoother, via `image`.
    Gaussian,
    /// Lanczos3 — the sharpest downscale, via `image`, and the most expensive.
    Lanczos3,
}

impl ScaleFilter {
    /// Short display label for UI/status output.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Nearest => "Nearest",
            Self::Bilinear => "Bilinear",
            Self::CatmullRom => "CatmullRom",
            Self::Gaussian => "Gaussian",
            Self::Lanczos3 => "Lanczos3",
        }
    }

    /// Whether this filter runs through `image::imageops::resize` rather than
    /// the crate's own kernels.
    pub fn is_image_kernel(&self) -> bool {
        matches!(self, Self::CatmullRom | Self::Gaussian | Self::Lanczos3)
    }
}

/// Audio source choice. Only [`AudioMode::None`] is supported: System/Both
/// need a loopback capture source the library does not provide (not
/// implemented), and Mic is explicitly todo — no microphone capture path
/// exists yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AudioMode {
    /// No audio track — the only supported mode.
    #[default]
    None,
    /// System (loopback) audio — not implemented; no loopback source exists
    /// in the library.
    System,
    /// Microphone audio — todo: no mic capture path exists yet.
    Mic,
    /// System + microphone — not implemented (needs the loopback source).
    Both,
}

impl AudioMode {
    /// Whether a capture can actually record this audio source. Only `None`.
    pub fn is_supported(&self) -> bool {
        matches!(self, Self::None)
    }
}

/// Everything needed to start a recording. Defaults mirror the library
/// defaults (spec: 15 Mbps is the library's default bitrate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureConfig {
    pub codec: Codec,
    /// Target bitrate in kilobits/s. The library honors it as a CBR target
    /// (`MF_MT_AVG_BITRATE`); it exposes no other rate-control mode.
    pub bitrate_kbps: u32,
    pub resolution: Resolution,
    /// Filter for the per-frame CPU resize; unused when the frame already
    /// matches the encoder target.
    pub scale_filter: ScaleFilter,
    /// Frames per second: the pacing interval, the encoder frame rate, and the
    /// replay-ring depth. `0` is unusable — see [`CaptureConfig::invalid_reason`].
    pub fps: u8,
    /// Pre-roll replay length in seconds; `replay_seconds()` clamps it.
    pub replay_seconds: u32,
    pub audio: AudioMode,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            codec: Codec::H264,
            bitrate_kbps: 15_000,
            resolution: Resolution::P1080,
            scale_filter: ScaleFilter::Bilinear,
            fps: 30,
            replay_seconds: 0,
            audio: AudioMode::None,
        }
    }
}

/// Upper bound for replay pre-roll, in seconds (memory stays bounded: see
/// `ring::estimated_memory_bytes`).
const MAX_REPLAY_SECONDS: u32 = 600;

impl CaptureConfig {
    /// Replay seconds clamped to 0..[`MAX_REPLAY_SECONDS`].
    pub fn replay_seconds(&self) -> u32 {
        self.replay_seconds.min(MAX_REPLAY_SECONDS)
    }

    /// Bitrate in bits/s, saturating so an extreme value never overflows.
    pub fn bitrate_bps(&self) -> u32 {
        self.bitrate_kbps.saturating_mul(1_000)
    }

    /// Exact frame interval: 1 s / fps in integer nanoseconds (floor, no float
    /// drift — the capture pump waits on this duration). `fps == 0` has no
    /// interval and is refused before capture ([`CaptureConfig::invalid_reason`]);
    /// the helper clamps it to 1 fps so nothing divides by zero.
    pub fn frame_interval(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.fps.max(1)))
    }

    /// Why this configuration cannot be captured, in Russian (user-facing), or
    /// `None` when it can. The check is deliberately tight: only the values no
    /// capture can use are refused — `fps == 0` has no pacing interval and no
    /// encoder frame rate, `Resolution::Height(0)` has no target size (a 0-row
    /// frame resizes and flips to empty bytes). Callers turn `Some` into
    /// [`crate::error::CaptureError::NotConfigured`].
    #[must_use]
    pub fn invalid_reason(&self) -> Option<&'static str> {
        if self.fps == 0 {
            return Some("частота кадров 0 недопустима: нужен fps от 1");
        }

        if matches!(self.resolution, Resolution::Height(0)) {
            return Some("высота разрешения 0 недопустима: нужна высота от 1 пикселя");
        }

        None
    }
}

#[cfg(test)]
#[path = "config.tests.rs"]
mod tests;
