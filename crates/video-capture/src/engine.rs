//! Capture engine + `Recorder` facade (Stage 2).
//!
//! The capture loop is Windows-only and runs inside `windows-capture`'s
//! capture thread (a Win32 message pump owned by the library). Everything here
//! is platform-independent *except* the handler body and the facade's Windows
//! internals, which are behind `#[cfg(windows)]`.
//!
//! **Shape.** The monitor is captured continuously from the moment
//! [`Recorder::new`] runs, so the pre-roll [`ReplayRing`] is always warm. A
//! recording is created *on demand*: `EngineCommand::Start` builds a
//! [`VideoEncoder`] at the configured target size **on the capture thread**
//! (WinRT is initialized there and `VideoEncoder::new` blocks ~1 s), then
//! **moves it into a dedicated encoder writer thread** ([`EncoderWriter`])
//! that owns the encoder outright. The writer replays the buffered frames
//! through `send_frame_buffer` and then consumes live frames from a bounded
//! queue ([`LIVE_QUEUE_FRAMES`]). The frame callback never blocks on encoder
//! work: during a recording it only copies BGRA bytes and pushes them with
//! `try_send`, so a slow encoder costs dropped frames instead of freezing
//! capture. `EngineCommand::Stop` asks the writer thread to finish (flush +
//! mux) the file; capture keeps running for the ring.
//!
//! **Frame feed while recording.** `Frame` is not `Send`, so the GPU-surface
//! fast path (`send_frame`) cannot leave the callback towards the writer
//! thread. Every live frame is therefore fed as owned BGRA bytes (padding
//! stripped, resized to the target when the sizes differ, rows flipped): the
//! size-match fast path is dropped while recording — an honest CPU copy,
//! documented here rather than hidden.
//!
//! **Commands & events.** The facade talks to the capture thread over plain
//! `std::sync::mpsc` channels: [`EngineCommand`] in, [`EngineEvent`] out. The
//! handler polls the command channel with non-blocking `try_recv` inside
//! `on_frame_arrived`; `Quit` calls `capture_control.stop()`, which halts the
//! frame pool and posts `WM_QUIT` so the thread returns. `SetReplay` is
//! fire-and-forget — it changes the ring capacity on the next captured frame
//! and is never acked (disabling it while nothing is recording also arms the
//! session release) — and the ring is fed only while no recording is running,
//! so a recording never holds a second copy of the frames. The facade exposes
//! that command as `Recorder::enable_replay`/`Recorder::disable_replay` and keeps the
//! requested state locally, so `Recorder::replay_enabled` answers without a
//! roundtrip to the capture thread. Encoder failures are traced and reported as
//! events instead of aborting the capture: the ring must survive a failed
//! recording.
//!
//! **Queued commands, polled acks.** `Recorder::start` and `Recorder::stop`
//! validate the path, queue [`EngineCommand::Start`] / [`EngineCommand::Stop`],
//! record a pending deadline ([`ACK_TIMEOUT`]), and return `Ok(())` — they never
//! block, so neither call can stall until the next delivered frame. The
//! acks come from different threads: `RecordingStarted` is reported by the
//! capture thread once the first live frame has been queued to the writer,
//! and `RecordingStopped` / `CommandFailed` for the finish are reported by the
//! writer thread after the file is muxed. The ~1 s cost of
//! `VideoEncoder::new` still lands inside one capture callback (WinRT is
//! per-thread, so the encoder cannot be created elsewhere without `unsafe`);
//! `finish()` runs on the writer thread, so muxing never freezes capture or
//! the UI. The writer also leaves a *done notice* on a separate channel
//! (`done_rx`) that the capture thread polls with
//! non-blocking `try_recv`: it retires the writer handle and re-checks the
//! session release, and because the notice is sent *after* the ack, the release
//! is reported on the following frame. The outcome is read with
//! `Recorder::poll_recording_ack`/`Recorder::poll_stop_ack`, which drain the
//! event channel with `try_recv`: `Some(Ok(()))` once the matching ack event
//! arrives, `Some(Err(..))` when a `CommandFailed` arrives or the pending
//! deadline elapses, `None` while still pending. An outcome is reported once
//! (the pending state is consumed), and asynchronous `FrameFailed` reports plus
//! the other command's acks are logged, never swallowed.
//! `Recorder::recording` is the boot helper for "record from the very first
//! captured frame": the command channel is built and `EngineCommand::Start` is
//! queued *before* `start_free_threaded`, so the first frame the thread delivers
//! starts the recording. `Recorder::new` stays the session boot that only warms
//! the pre-roll ring.
//!
//! **Pacing.** `MinimumUpdateIntervalSettings::Custom` is only a hint — the
//! API does not guarantee a constant rate — so the handler drops frames whose
//! timestamp delta from the last *accepted* frame is below the configured frame
//! interval ([`should_send`]). The baseline advances only on frames that are
//! actually fed to the encoder, which keeps the output rate at the configured
//! fps instead of starving it: frames captured between `Recorder::new` and
//! `EngineCommand::Start` fill the ring rather than the encoder, so they leave
//! the clock alone and the first frame after a recording is created is always
//! sent.
//! The hint itself is optional: some OS builds have no `MinUpdateInterval`
//! WinRT property, and asking for it there fails the whole capture start, so
//! the facade probes `GraphicsCaptureApi::is_minimum_update_interval_supported`
//! before building `Settings` and drops the hint in favour of timestamp pacing
//! when it is unavailable.
//!
//! **Debug dumps.** On Windows, `Recorder::with_debug_dump` (or the
//! `VC_DEBUG_DUMP_DIR` environment variable) writes the first three frames that
//! go through the raw-buffer path as `frame_<index>_raw.png`,
//! `frame_<index>_flipped.png`, and `frame_<index>_rgba.png` before they are
//! queued to the writer, so the exact bytes fed to `send_frame_buffer` can be
//! inspected (row order, stride, channel interpretation). While recording is
//! the only user of the raw-buffer path, so the dumps cover the first three
//! recorded frames whatever the captured size. Failures are warnings only.
//!
//! **Not implemented.** `AudioMode::System`/`Mic`/`Both`: the library exposes
//! no audio capture source, so the audio track is always disabled
//! (`AudioSettingsBuilder::disabled(true)`). Encoder vendor selection and
//! CQP/CRF bitrate modes are not reachable through this library (see the crate
//! docs); resolution changes are done by a CPU BGRA resize
//! ([`resize_bgra`]) at the desktop's aspect ratio — the preset fixes the
//! height and the width follows it — never by cropping/padding inside the
//! encoder.

use std::path::{Path, PathBuf};
use std::time::Instant;

use image::imageops::{FilterType, resize};
use image::{ImageBuffer, Rgba};

use crate::config::CaptureConfig;
use crate::config::ScaleFilter;
use crate::error::CaptureError;
use crate::ring::frame_bytes;

#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::sync::mpsc::{self, TrySendError};
#[cfg(windows)]
use std::thread::JoinHandle;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use image::ExtendedColorType;
#[cfg(windows)]
use image::ImageEncoder;
#[cfg(windows)]
use image::codecs::png::PngEncoder;

#[cfg(windows)]
use crate::config::Codec;
#[cfg(windows)]
use crate::ring::ReplayRing;
#[cfg(windows)]
use crate::ring::estimated_memory_bytes;

#[cfg(windows)]
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
#[cfg(windows)]
use windows_capture::encoder::{
    AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
    VideoSettingsSubType,
};
#[cfg(windows)]
use windows_capture::frame::Frame;
#[cfg(windows)]
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
#[cfg(windows)]
use windows_capture::monitor::Monitor;
#[cfg(windows)]
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

/// How long a queued command may stay pending before `poll_recording_ack` /
/// `poll_stop_ack` report a timeout. The thread answers on frame arrival, so a
/// desktop that produces no frames (or a capture that died) surfaces as an error
/// instead of a request that stays pending forever.
#[cfg(windows)]
const ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// How many raw-buffer frames the debug dump writes (one PNG set per frame).
#[cfg(windows)]
const DEBUG_DUMP_FRAMES: usize = 3;

/// Debug dump directory from `VC_DEBUG_DUMP_DIR`: unset or empty means none.
///
/// Reading the variable is safe; *setting* it from the demo would need
/// `unsafe fn` in edition 2024, which this crate forbids, so the demo passes
/// the directory through [`Recorder::with_debug_dump`] and this fallback only
/// serves an environment set outside the process.
#[cfg(windows)]
fn debug_dump_dir_from_env() -> Option<PathBuf> {
    std::env::var("VC_DEBUG_DUMP_DIR")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Write `bytes` verbatim as a PNG at `path`, declared as `Rgba8`.
///
/// The BGRA buffer is stored in `Rgba<u8>` slots *without* a channel swap
/// (`ImageBuffer::from_raw`, not `from_raw_bgra`), so the PNG shows exactly the
/// bytes the encoder received: red and blue appear swapped relative to the
/// screen, and any stride/row misreading shows up as visible garbage.
///
/// Fails instead of panicking: `ImageEncoder::write_image` asserts on the
/// buffer length, so a size mismatch is turned into an error here (the capture
/// thread must never panic).
#[cfg(windows)]
fn write_dump_png(path: &Path, width: u32, height: u32, bytes: &[u8]) -> Result<(), CaptureError> {
    if width == 0 || height == 0 || bytes.len() != frame_bytes(width, height) {
        return Err(CaptureError::Encoder(format!(
            "debug dump buffer is {} bytes for {width}x{height}",
            bytes.len()
        )));
    }

    let Some(image) = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(width, height, bytes.to_vec())
    else {
        return Err(CaptureError::Encoder(format!(
            "debug dump buffer rejected for {width}x{height}"
        )));
    };

    let file = File::create(path)?;

    PngEncoder::new(file)
        .write_image(image.as_raw(), width, height, ExtendedColorType::Rgba8)
        .map_err(|error| CaptureError::Encoder(format!("debug PNG encode failed: {error}")))?;

    Ok(())
}

/// A facade-to-capture-thread command.
#[derive(Debug)]
pub enum EngineCommand {
    /// Start recording to `path` (encoder created on the capture thread).
    Start(PathBuf),
    /// Finish the current recording; capture keeps running.
    Stop,
    /// Change the pre-roll replay window to `seconds` (`0` = buffer disabled).
    ///
    /// Applied on the next captured frame, fire-and-forget: the capture thread
    /// never acks it, so the sender does not wait. Enabling rebuilds the ring
    /// capacity from the clamped seconds (`CaptureConfig::replay_seconds`,
    /// `frames = seconds * fps`) and keeps the frames already buffered up to
    /// the new window; disabling drains the ring and releases its buffers.
    /// Because the ring is not fed while a recording is active, enabling during
    /// a recording starts filling the ring only after recording stops.
    SetReplay(u32),
    /// Finish the recording (if any) and stop the capture thread.
    Quit,
}

/// A capture-thread report. Command acks (`RecordingStarted`/`RecordingStopped`
/// /`CommandFailed`) are what the facade polls for (`Recorder::poll_recording_ack`
/// /`Recorder::poll_stop_ack`); `FrameFailed` is asynchronous and only says
/// "this frame was dropped", never "the command failed".
#[derive(Debug)]
pub enum EngineEvent {
    /// The first live frame was queued to the encoder writer thread; the
    /// recording (pre-roll replay plus live feed) is live.
    RecordingStarted {
        /// Output file the recording is being written to.
        path: PathBuf,
    },
    /// The encoder writer thread finished the encoder; the file is muxed and
    /// closed. Reported by the writer thread, never by the capture callback.
    RecordingStopped,
    /// Nothing needs frames any more — a recording finished while the replay
    /// ring had no capacity, or the replay buffer was disabled while no
    /// recording was running — so the capture session was released; the facade
    /// treats further commands as `NotConfigured`.
    CaptureReleased,
    /// A command was rejected or failed (path validation, encoder create/finish).
    CommandFailed {
        /// The typed failure, already traced by the engine.
        error: CaptureError,
    },
    /// A single frame could not be read or sent; capture continues.
    FrameFailed {
        /// The typed failure, already traced by the engine.
        error: CaptureError,
    },
}

/// Facade-side bookkeeping for one queued command that awaits its ack.
///
/// The state exists on every platform: the Windows facade constructs the
/// pending/acked/failed states when a command is queued and its events arrive,
/// and the Linux facade keeps `Idle` because nothing can be queued there.
/// [`ack_outcome`] is the single decision point for both, so the Linux build is
/// exempt from the never-constructed lint (the tests exercise every state).
#[derive(Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
enum AckState {
    /// Nothing queued, or the outcome has already been reported.
    Idle,
    /// Command queued on the channel; its ack is awaited until `deadline`.
    Pending { deadline: Instant },
    /// The capture thread reported success (`RecordingStarted`/`RecordingStopped`).
    Acked,
    /// The capture thread reported a typed failure (`CommandFailed`).
    Failed(CaptureError),
}

/// Outcome of one non-blocking ack poll, evaluated at the injected `now`.
///
/// `command` labels the timeout message (`"start"` / `"stop"`); `now` is
/// injected so the deadline boundary is testable instead of tied to a wall
/// clock. The reported state is consumed (reset to `Idle`), so an ack or a
/// failure is reported exactly once and a second poll of the same command
/// returns `None`; a still-pending deadline is kept.
///
/// - `Idle` → `None` (nothing pending; the Linux stub always lands here)
/// - `Pending` before `deadline` → `None` (still pending)
/// - `Pending` at/after `deadline` → `Some(Err(Encoder(..)))` timeout
/// - `Acked` → `Some(Ok(()))`
/// - `Failed(error)` → `Some(Err(error))`
fn ack_outcome(
    command: &str,
    pending: &mut AckState,
    now: Instant,
) -> Option<Result<(), CaptureError>> {
    let state = std::mem::replace(pending, AckState::Idle);

    match state {
        AckState::Idle => None,
        AckState::Acked => Some(Ok(())),
        AckState::Failed(error) => Some(Err(error)),
        AckState::Pending { deadline } => {
            if now < deadline {
                *pending = AckState::Pending { deadline };
                return None;
            }

            tracing::error!(command, "capture thread did not acknowledge the command");
            Some(Err(CaptureError::Encoder(format!(
                "capture thread did not acknowledge the {command} request"
            ))))
        }
    }
}

/// Output-path validation, shared by the facade and the engine so a bad path is
/// rejected identically on both sides.
///
/// Rejects an empty path and a path whose (non-empty) parent is not an existing
/// directory: the encoder's `File::create` needs the parent to exist, so a
/// missing directory is reported here rather than deeper in the pipeline.
pub fn validate_output_path(path: &Path) -> Result<(), CaptureError> {
    if path.as_os_str().is_empty() {
        return Err(CaptureError::InvalidPath(
            "output path is empty".to_string(),
        ));
    }

    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent
        && !parent.is_dir()
    {
        return Err(CaptureError::InvalidPath(format!(
            "output directory does not exist: {}",
            parent.display()
        )));
    }

    Ok(())
}

/// Refuse a configuration no capture can use. The reason comes from
/// [`CaptureConfig::invalid_reason`] — Russian, user-facing — is logged as an
/// error, and is reported as [`CaptureError::NotConfigured`]: a typed failure,
/// never a panic. Shared by every `Recorder` constructor on both platforms, so
/// an unusable config is refused before any platform check or capture thread.
fn reject_invalid_config(config: &CaptureConfig) -> Result<(), CaptureError> {
    let Some(reason) = config.invalid_reason() else {
        return Ok(());
    };

    tracing::error!(reason, "recorder refused: configuration is unusable");
    Err(CaptureError::NotConfigured)
}

/// Resize raw 4-bytes-per-pixel frame bytes to `dst_w x dst_h` with `filter`.
///
/// The bytes are BGRA (the capture pool is created with `ColorFormat::Bgra8`,
/// which is also what `VideoEncoder::send_frame_buffer` expects). They are
/// handed to `image` as `Rgba<u8>`: every filter moves whole pixels, so the R/B
/// order inside each pixel is preserved verbatim and never swapped.
///
/// `Nearest` and `Bilinear` run through this crate's kernels (`nearest_bgra`,
/// `bilinear_bgra`) because a live capture loop needs the per-frame cost to stay
/// small; `image::imageops::resize` costs tens of milliseconds per frame even
/// for `Nearest`. The `image`-backed filters are quality options, and their
/// measured cost is documented in the README.
///
/// Returns an empty vector when either dimension is `0` or `bytes` is not
/// exactly `src_w * src_h * 4` — the engine only ever passes frame-sized
/// buffers, so a mismatch means a caller bug and is surfaced as an empty result.
pub fn resize_bgra(
    bytes: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    filter: ScaleFilter,
) -> Vec<u8> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 {
        return Vec::new();
    }

    if bytes.len() != frame_bytes(src_w, src_h) {
        return Vec::new();
    }

    if src_w == dst_w && src_h == dst_h {
        return bytes.to_vec();
    }

    match filter {
        ScaleFilter::Nearest => nearest_bgra(bytes, src_w, src_h, dst_w, dst_h),
        ScaleFilter::Bilinear => bilinear_bgra(bytes, src_w, src_h, dst_w, dst_h),
        ScaleFilter::CatmullRom | ScaleFilter::Gaussian | ScaleFilter::Lanczos3 => {
            image_resize(bytes, src_w, src_h, dst_w, dst_h, filter)
        }
    }
}

/// Nearest-neighbor pick: destination pixel `(x, y)` takes source pixel
/// `(x * src_w / dst_w, y * src_h / dst_h)`, clamped to the source bounds.
fn nearest_bgra(bytes: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let mut out = vec![0u8; frame_bytes(dst_w, dst_h)];

    for dst_y in 0..dst_h as usize {
        let src_y = (dst_y as u64 * u64::from(src_h) / u64::from(dst_h)) as usize;
        let src_y = src_y.min(src_h as usize - 1);
        let src_row = src_y * src_w as usize * 4;
        let dst_row = dst_y * dst_w as usize * 4;

        for dst_x in 0..dst_w as usize {
            let src_x = (dst_x as u64 * u64::from(src_w) / u64::from(dst_w)) as usize;
            let src_x = src_x.min(src_w as usize - 1);

            out[dst_row + dst_x * 4..dst_row + dst_x * 4 + 4]
                .copy_from_slice(&bytes[src_row + src_x * 4..src_row + src_x * 4 + 4]);
        }
    }

    out
}

/// Separable bilinear: interpolate every row horizontally to `dst_w`, then
/// interpolate the resulting rows vertically to `dst_h`. Linear weights keep
/// thin glyph strokes that `nearest_bgra` drops.
///
/// The weights are integer (0..256) and precomputed once per axis, so the inner
/// loops are integer blends over bytes rather than per-pixel float sampling —
/// this is the reason the crate kernel beats `image::imageops::resize` here.
fn bilinear_bgra(bytes: &[u8], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<u8> {
    let wide = interpolate_rows_x(bytes, src_w, src_h, dst_w);
    interpolate_rows_y(&wide, src_h, dst_h, dst_w)
}

/// Sample position of a destination pixel center in source coordinates, plus
/// the two source indices it lies between and the 8-bit blend weight.
fn sample_positions(src: u32, dst: u32) -> Vec<(u32, u32, u16)> {
    let max = src as f32 - 1.0;

    (0..dst)
        .map(|dst_index| {
            // Pixel centers, clamped to the last pixel so edges never
            // extrapolate past the source.
            let position =
                ((dst_index as f32 + 0.5) * src as f32 / dst as f32 - 0.5).clamp(0.0, max);
            let low = position as u32;
            let high = low + 1;
            let weight = ((position - low as f32) * 256.0).round() as u16;

            (low, high.min(src - 1), weight)
        })
        .collect()
}

/// Integer blend of two channel values with an 8-bit weight (round-to-nearest).
/// The shift is parenthesized deliberately: Rust binds `+` tighter than `>>`,
/// so `a + w >> 8` would shift the whole sum instead of the weighted delta.
fn blend(a: u8, b: u8, weight: u16) -> u8 {
    let delta = (b as i32 - a as i32) * weight as i32 + 128;

    (a as i32 + (delta >> 8)) as u8
}

/// Horizontal pass: `src_w x src_h` -> `dst_w x src_h`, one row at a time.
fn interpolate_rows_x(bytes: &[u8], src_w: u32, src_h: u32, dst_w: u32) -> Vec<u8> {
    let samples = sample_positions(src_w, dst_w);
    let mut wide = vec![0u8; frame_bytes(dst_w, src_h)];

    for row in 0..src_h as usize {
        let src_row = row * src_w as usize * 4;
        let dst_row = row * dst_w as usize * 4;

        for (dst_x, sample) in samples.iter().enumerate() {
            let (low, high, weight) = *sample;
            let a = &bytes[src_row + low as usize * 4..src_row + low as usize * 4 + 4];
            let b = &bytes[src_row + high as usize * 4..src_row + high as usize * 4 + 4];
            let out = &mut wide[dst_row + dst_x * 4..dst_row + dst_x * 4 + 4];

            out[0] = blend(a[0], b[0], weight);
            out[1] = blend(a[1], b[1], weight);
            out[2] = blend(a[2], b[2], weight);
            out[3] = blend(a[3], b[3], weight);
        }
    }

    wide
}

/// Vertical pass: a `dst_w x src_h` buffer -> `dst_w x dst_h`, blending whole
/// rows (the horizontal pass already brought them to `dst_w`).
fn interpolate_rows_y(wide: &[u8], src_h: u32, dst_h: u32, dst_w: u32) -> Vec<u8> {
    let samples = sample_positions(src_h, dst_h);
    let row_bytes = dst_w as usize * 4;
    let mut out = vec![0u8; frame_bytes(dst_w, dst_h)];

    for (dst_y, sample) in samples.iter().enumerate() {
        let (low, high, weight) = *sample;
        let row0 = &wide[low as usize * row_bytes..(low as usize + 1) * row_bytes];
        let row1 = &wide[high as usize * row_bytes..(high as usize + 1) * row_bytes];
        let out_row = &mut out[dst_y * row_bytes..(dst_y + 1) * row_bytes];

        if weight == 0 {
            out_row.copy_from_slice(row0);
        } else if weight == 256 {
            out_row.copy_from_slice(row1);
        } else {
            for (offset, byte) in out_row.iter_mut().enumerate() {
                *byte = blend(row0[offset], row1[offset], weight);
            }
        }
    }

    out
}

/// `image::imageops::resize` for the quality filters, BGRA-as-Rgba bytes.
fn image_resize(
    bytes: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    filter: ScaleFilter,
) -> Vec<u8> {
    let Some(source) = ImageBuffer::<Rgba<u8>, &[u8]>::from_raw(src_w, src_h, bytes) else {
        return Vec::new();
    };

    let kernel = match filter {
        ScaleFilter::CatmullRom => FilterType::CatmullRom,
        ScaleFilter::Gaussian => FilterType::Gaussian,
        ScaleFilter::Lanczos3 => FilterType::Lanczos3,
        ScaleFilter::Nearest | ScaleFilter::Bilinear => FilterType::Nearest,
    };

    resize(&source, dst_w, dst_h, kernel).into_raw()
}

/// Vertically flip raw BGRA frame bytes: output row `i` is input row
/// `height - 1 - i`, pixels and their BGRA channel order stay verbatim.
///
/// The orientation convention, verified on a Windows host by the debug dumps:
/// the D3D staging buffer of a captured frame is top-to-bottom (row 0 is the
/// top of the screen), while the raw buffer path of `send_frame_buffer`
/// expects bottom-to-top BGRA and renders top-to-bottom bytes upside down.
/// So this flip is part of the feed, not only a debug aid: the Windows engine
/// applies it to everything sent through that path, and a debug dump can still
/// show the upright variant next to the bytes actually fed.
///
/// Returns an empty vector when either dimension is `0` or `bytes` is not
/// exactly `width * height * 4` — same contract as [`resize_bgra`].
pub fn flip_rows_bgra(bytes: &[u8], width: u32, height: u32) -> Vec<u8> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    if bytes.len() != frame_bytes(width, height) {
        return Vec::new();
    }

    let row_bytes = frame_bytes(width, 1);
    let mut flipped = vec![0u8; bytes.len()];

    for row in 0..height as usize {
        let source = row * row_bytes;
        let destination = (height as usize - 1 - row) * row_bytes;
        flipped[destination..destination + row_bytes]
            .copy_from_slice(&bytes[source..source + row_bytes]);
    }

    flipped
}

/// Whether a frame with timestamp `next_ts` (100 ns ticks) should be written,
/// given the timestamp of the last accepted frame.
///
/// Frames closer than `interval_ticks` to the previous accepted frame are
/// dropped; the caller advances `last_ts` only on accepted frames, so a burst of
/// fast frames cannot starve the recording. A frame with an earlier timestamp
/// than `last_ts` (clock jitter) is dropped rather than re-timed.
///
/// The caller consults this only while an encoder exists (`on_frame_arrived`):
/// frames captured before a recording starts are buffered for the replay ring,
/// not fed to an encoder, so they are never paced and never move `last_ts`.
pub fn should_send(next_ts: i64, last_ts: Option<i64>, interval_ticks: i64) -> bool {
    let Some(last_ts) = last_ts else {
        return true;
    };

    next_ts.saturating_sub(last_ts) >= interval_ticks
}

/// Frame interval in the capture API's 100 ns tick unit.
pub fn interval_ticks(config: &CaptureConfig) -> i64 {
    i64::try_from(config.frame_interval().as_nanos() / 100).unwrap_or(i64::MAX)
}

/// Ring capacity implied by the configuration: clamped replay seconds x fps.
/// `replay_seconds == 0` therefore means the ring stores nothing.
pub fn ring_capacity(config: &CaptureConfig) -> usize {
    crate::ring::frames_for(config.replay_seconds(), u32::from(config.fps))
}

/// Whether the capture session has no reason to stay open: nothing will ever
/// be buffered again — the replay ring stores no frames (`ring_capacity == 0`)
/// **and** no recording is in progress. Either reason on its own keeps the
/// session open: a live recording needs frames from it, and a ring that still
/// wants frames needs the session to keep producing them.
///
/// Consumed by the Windows done-notice poll (a recording just finished on the
/// writer thread) and by `set_replay` (the replay buffer was disabled while no
/// recording runs); the Linux build has no caller, so it is exempt from the
/// never-used lint there (the tests exercise all four capacity/recording
/// combinations).
#[cfg_attr(not(windows), allow(dead_code))]
fn session_should_release(ring_capacity: usize, recording_active: bool) -> bool {
    ring_capacity == 0 && !recording_active
}

/// Whether an event reports that the capture session was released (the
/// capture thread called `capture_control.stop()` because nothing needed
/// frames any more: no recording and a replay ring with no capacity).
///
/// The release is **not an ack for either command**: it neither completes a
/// queued `Start` nor a queued `Stop`, so the facade records it separately
/// (`Recorder::session_released`) instead of consuming a pending ack.
///
/// Consumed by the Windows `drain_events`; the Linux build has no caller, so
/// it is exempt from the never-used lint there (the tests cover every event).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn event_is_capture_release(event: &EngineEvent) -> bool {
    matches!(event, EngineEvent::CaptureReleased)
}

/// One-line dump of every parameter a recording is asked for, plus a marker for
/// the one choice the library cannot honor.
///
/// Debug names are used where `config` carries no display label (`codec`,
/// `resolution`, `audio`); `scale_filter` uses [`crate::config::ScaleFilter::label`].
/// An unsupported `audio` is appended as a marker so a log shows what was
/// *requested but not applied* instead of silently reporting the library
/// default. `replay_seconds` is the clamped value the engine actually uses.
///
/// Rate control is CBR only — the library exposes a bitrate value, not modes —
/// so no mode appears in the dump, and `CaptureConfig` has no encoder field:
/// encoder vendor selection is not reachable through the library.
#[must_use]
pub fn config_dump(config: &CaptureConfig) -> String {
    let mut dump = format!(
        "codec={:?} bitrate_kbps={} resolution={:?} scale_filter={} fps={} replay_seconds={} audio={:?}",
        config.codec,
        config.bitrate_kbps,
        config.resolution,
        config.scale_filter.label(),
        config.fps,
        config.replay_seconds(),
        config.audio,
    );

    if !config.audio.is_supported() {
        dump.push_str(&format!(
            " audio={:?} (unsupported — audio capture is not implemented, none only)",
            config.audio
        ));
    }

    dump
}

/// Minimum-update-interval choice for `Settings`, plus a label for the log.
///
/// `Custom` makes the library call `GraphicsCaptureSession::SetMinUpdateInterval`
/// and fail the whole capture start with
/// `Error::MinimumUpdateIntervalUnsupported` when the WinRT `MinUpdateInterval`
/// property is absent on the OS build, so the capability is probed *before*
/// `Settings` is built. `Ok(false)` and a failed probe both fall back to
/// `Default`: timestamp pacing in `on_frame_arrived` (`should_send`) already
/// caps delivery at the configured fps, so the fallback costs only the hint.
#[cfg(windows)]
fn minimum_update_interval(
    config: &CaptureConfig,
) -> (MinimumUpdateIntervalSettings, &'static str) {
    match GraphicsCaptureApi::is_minimum_update_interval_supported() {
        Ok(true) => (
            MinimumUpdateIntervalSettings::Custom(config.frame_interval()),
            "custom",
        ),
        Ok(false) => {
            tracing::warn!(
                "minimum update interval unsupported on this platform — fps pacing \
                 falls back to timestamp-based should_send"
            );
            (MinimumUpdateIntervalSettings::Default, "default")
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "minimum update interval probe failed — fps pacing falls back to \
                 timestamp-based should_send"
            );
            (MinimumUpdateIntervalSettings::Default, "default")
        }
    }
}

/// Bootstrap values passed through `Settings::new(.., flags)` into
/// `CaptureEngine::new`, so the engine gets its channels and desktop size
/// without the facade touching the capture thread.
#[cfg(windows)]
struct EngineFlags {
    config: CaptureConfig,
    /// Native desktop size of the captured monitor.
    native: (u32, u32),
    /// `"custom"` or `"default"`: the minimum-update-interval mode `Settings`
    /// was built with, recorded by the facade after the capability probe.
    interval_mode: &'static str,
    /// Debug dump directory for the raw-buffer path, already resolved by the
    /// facade (constructor argument, falling back to `VC_DEBUG_DUMP_DIR`).
    dump_dir: Option<PathBuf>,
    command_rx: mpsc::Receiver<EngineCommand>,
    event_tx: mpsc::Sender<EngineEvent>,
}

/// Bounded depth of the live-frame queue handed to the encoder writer thread.
///
/// A live frame is megabytes of BGRA at the target size (about 3.3 MB at 720p,
/// ~8.3 MB at native desktop size), so an unbounded queue would turn a lagging
/// encoder into unbounded RAM. The capture thread pushes with `try_send`: a
/// full queue drops the frame, so a writer that cannot keep up costs dropped
/// frames instead of memory.
#[cfg(windows)]
const LIVE_QUEUE_FRAMES: usize = 8;

/// Capture-thread handle to the dedicated encoder writer thread.
///
/// Owns nothing of the encoder: the encoder moves into the writer thread, which
/// owns it outright (`VideoEncoder` is `Send`, not `Sync`, so exactly one thread
/// may hold it). Dropping this handle closes `live_tx`, which is how
/// [`EngineCommand::Stop`] tells the writer to flush what is queued, mux the
/// file, and report the ack; the done notice then clears the handle on the
/// capture thread.
#[cfg(windows)]
struct EncoderWriter {
    /// Bounded queue of live frames — `(timestamp_ticks, BGRA bytes at the
    /// target size)` — fed with non-blocking `try_send`.
    live_tx: mpsc::SyncSender<(i64, Vec<u8>)>,
    /// The writer thread. Detached on drop (dropping a `JoinHandle` neither
    /// joins nor aborts it), because a frame callback must never block on
    /// encoder work; read only for diagnostics (`is_finished`).
    thread: JoinHandle<()>,
}

/// Body of the encoder writer thread: pre-roll replay, then the live queue,
/// then `finish()`.
///
/// Runs off the capture thread because `VideoEncoder::finish` blocks until the
/// transcode thread muxes the file. Timestamps stay monotonic by construction:
/// the ring hands frames in capture order (oldest first) and every live frame
/// is pushed after `Start`, so feeding FIFO keeps Media Foundation's
/// monotonic-timestamp requirement.
///
/// Reports the outcome on the event channel — `RecordingStopped` on success,
/// one `CommandFailed` on any failure — and then leaves the done notice
/// (`generation`) so the capture thread can drop the stale handle and decide
/// whether the session still has a reason to stay open.
#[cfg(windows)]
fn run_encoder_writer(
    mut encoder: VideoEncoder,
    path: PathBuf,
    generation: u64,
    pre_roll: Vec<(i64, Vec<u8>)>,
    live_rx: mpsc::Receiver<(i64, Vec<u8>)>,
    done_tx: mpsc::Sender<u64>,
    event_tx: mpsc::Sender<EngineEvent>,
) {
    let mut failure: Option<CaptureError> = None;

    for (timestamp, bytes) in pre_roll {
        if let Err(error) = encoder.send_frame_buffer(&bytes, timestamp) {
            tracing::error!(path = %path.display(), %error, "replay frame rejected");
            failure = Some(CaptureError::Encoder(format!(
                "replay frame send failed: {error}"
            )));
            break;
        }
    }

    // Blocking `recv` is fine here: this thread is allowed to wait for frames,
    // the capture callback is not. The queue closes when the engine drops the
    // writer handle, so a `Stop` ends the loop after the queued frames drain.
    if failure.is_none() {
        for (timestamp, bytes) in live_rx {
            if let Err(error) = encoder.send_frame_buffer(&bytes, timestamp) {
                tracing::error!(
                    path = %path.display(),
                    %error,
                    "encoder frame send failed; recording stopped"
                );
                failure = Some(CaptureError::Encoder(format!(
                    "encoder frame send failed: {error}"
                )));
                break;
            }
        }
    }

    // A failed feed leaves the encoder dropped instead of muxed: `finish()` on
    // a dead encoder would report a second failure for the same recording.
    if failure.is_none() {
        match encoder.finish() {
            Ok(()) => {
                tracing::info!(path = %path.display(), "recording finished");
                if event_tx.send(EngineEvent::RecordingStopped).is_err() {
                    tracing::warn!("event channel closed; stop ack dropped");
                }
            }
            Err(error) => {
                tracing::error!(path = %path.display(), %error, "encoder finish failed");
                failure = Some(CaptureError::Encoder(format!(
                    "encoder finish failed: {error}"
                )));
            }
        }
    }

    if let Some(error) = failure
        && event_tx.send(EngineEvent::CommandFailed { error }).is_err()
    {
        tracing::warn!("event channel closed; failure report dropped");
    }

    if done_tx.send(generation).is_err() {
        tracing::warn!("done channel closed; finish notice dropped");
    }
}

/// The capture handler: always-on monitor capture, replay ring, and a
/// recording that exists only between `Start` and `Stop`.
///
/// Lives on the library's capture thread. The handler owns the *writer handle*
/// only: the encoder itself is moved into the encoder writer thread, which is
/// its sole owner (`VideoEncoder` is `Send` but not `Sync`). The handler never
/// blocks on encoder work — live frames go to a bounded queue via `try_send`,
/// and the outcome comes back as an event plus a done notice.
#[cfg(windows)]
struct CaptureEngine {
    config: CaptureConfig,
    /// Encoder/ring size: the configured resolution within the native size.
    target: (u32, u32),
    ring: ReplayRing,
    ring_capacity: usize,
    /// Set by [`session_should_release`] when a finished recording or a
    /// disabled replay buffer leaves nothing to capture for. The next frame
    /// consumes it and releases the session instead of handling the frame.
    release_session: bool,
    /// Writer handle for the recording between `Start` and `Stop`; `None` when
    /// no recording is running.
    writer: Option<EncoderWriter>,
    /// Output path whose `RecordingStarted` ack is still owed: `Some` until the
    /// first live frame has been queued to the writer, and taken by that ack.
    recording_path: Option<PathBuf>,
    /// Stamp of the current writer's done notice. A `Stop` followed by a `Start`
    /// in one callback leaves the old writer finishing, so a done notice is
    /// matched against this stamp before the handle is cleared.
    writer_generation: u64,
    /// Done notices from writer threads (the generation of the recording that
    /// just finished), polled non-blocking in `on_frame_arrived`.
    done_rx: mpsc::Receiver<u64>,
    /// Sending half kept by the engine: every writer gets a clone, so the done
    /// channel stays open between recordings.
    done_tx: mpsc::Sender<u64>,
    command_rx: mpsc::Receiver<EngineCommand>,
    event_tx: mpsc::Sender<EngineEvent>,
    /// Reusable scratch for the padding-stripped frame buffer, kept across
    /// frames so the capture loop does not allocate per frame.
    scratch: Vec<u8>,
    last_timestamp: Option<i64>,
    interval_ticks: i64,
    /// Directory for the debug PNG dumps (`None` = dumping disabled).
    dump_dir: Option<PathBuf>,
    /// Raw-buffer frames still to dump; counts down from [`DEBUG_DUMP_FRAMES`].
    dump_remaining: usize,
}

#[cfg(windows)]
impl GraphicsCaptureApiHandler for CaptureEngine {
    type Flags = EngineFlags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let flags = ctx.flags;
        let target = flags.config.resolution.target_size(flags.native);
        let ring_capacity = ring_capacity(&flags.config);
        let interval_ticks = interval_ticks(&flags.config);
        // One done channel for the whole session: the engine keeps both halves
        // and hands a clone of the sender to each writer thread.
        let (done_tx, done_rx) = mpsc::channel();
        let dump_remaining = if flags.dump_dir.is_some() {
            DEBUG_DUMP_FRAMES
        } else {
            0
        };

        tracing::info!(
            native = ?flags.native,
            target = ?target,
            scaled = flags.native != target,
            scale_filter = flags.config.scale_filter.label(),
            ring_capacity,
            fps = flags.config.fps,
            replay_seconds = flags.config.replay_seconds(),
            interval = flags.interval_mode,
            dump_dir = ?flags.dump_dir,
            "capture engine created"
        );

        // The dump directory is created here (best-effort): a missing directory
        // must only cost warnings, never the capture.
        if let Some(dir) = &flags.dump_dir {
            if let Err(error) = std::fs::create_dir_all(dir) {
                tracing::warn!(
                    %error,
                    dir = %dir.display(),
                    "debug dump directory unavailable"
                );
            }
            tracing::info!(
                dir = %dir.display(),
                frames = DEBUG_DUMP_FRAMES,
                "debug frame dumps enabled"
            );
        }

        Ok(Self {
            config: flags.config,
            target,
            ring: ReplayRing::new(ring_capacity),
            ring_capacity,
            release_session: false,
            writer: None,
            recording_path: None,
            writer_generation: 0,
            done_rx,
            done_tx,
            command_rx: flags.command_rx,
            event_tx: flags.event_tx,
            scratch: Vec::new(),
            last_timestamp: None,
            interval_ticks,
            dump_dir: flags.dump_dir,
            dump_remaining,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        capture_control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        // Non-blocking command poll: the capture thread must never park here.
        let mut quit = false;
        while let Ok(command) = self.command_rx.try_recv() {
            match command {
                EngineCommand::Start(path) => self.start_recording(path),
                EngineCommand::Stop => self.stop_recording(),
                EngineCommand::SetReplay(seconds) => self.set_replay(seconds),
                EngineCommand::Quit => quit = true,
            }
        }

        if quit {
            self.stop_recording();
            capture_control.stop();
            return Ok(());
        }

        // Done notices from writer threads: the writer already reported the
        // ack or the failure on the event channel, so this only retires the
        // handle. The generation stamp keeps a notice from a recording that is
        // still finishing (a `Stop` followed by a new `Start` in this callback)
        // from retiring the *current* writer. Because the writer's ack is sent
        // before its notice, the release below always lands after the ack.
        while let Ok(generation) = self.done_rx.try_recv() {
            if self.writer_generation == generation {
                self.detach_writer();
            }

            self.release_session =
                session_should_release(self.ring_capacity, self.writer.is_some());
            if self.release_session {
                tracing::info!("replay ring disabled; capture session released");
            }
        }

        // An armed release is handled on the frame *after* the recording was
        // finished, so the stop ack was already reported before the session
        // closes; the frame itself is skipped, never blocked.
        if self.release_session {
            self.release_session = false;
            self.report(EngineEvent::CaptureReleased);
            capture_control.stop();
            return Ok(());
        }

        let timestamp = match frame.timestamp() {
            Ok(timestamp) => timestamp.Duration,
            Err(error) => {
                self.report_frame_failure(CaptureError::Encoder(format!(
                    "frame timestamp failed: {error}"
                )));
                return Ok(());
            }
        };

        // Pacing applies only to frames that reach the encoder. A frame that
        // arrives while no writer exists goes to the replay ring (or is
        // dropped), not to the encoder, so it must not consume the pacing
        // budget: `last_timestamp` stays untouched until a frame is actually
        // fed, otherwise frames captured between `Recorder::new` and the
        // `Start` command would move the clock and the frame that handles
        // `Start` could be dropped for up to one frame interval. With the
        // clock unset, that frame is accepted by the first-frame rule.
        let recording = self.writer.is_some();

        if recording {
            if !should_send(timestamp, self.last_timestamp, self.interval_ticks) {
                return Ok(());
            }
            self.last_timestamp = Some(timestamp);
        }

        let frame_size = (frame.width(), frame.height());
        let target = self.target;

        // Raw bytes are needed for the ring, but only while no recording is
        // running: during a recording the ring is not fed (RAM saving — the
        // pre-roll snapshot was already taken), so materializing them for it
        // would be pure waste. While recording they are needed for *every*
        // frame: `Frame` is not `Send`, so the GPU-surface path (`send_frame`)
        // cannot leave the callback towards the writer thread, and the writer
        // is fed owned BGRA bytes at the target size instead.
        let ring_wanted = self.ring_capacity > 0 && !recording;
        let needs_raw_bytes = ring_wanted || recording;
        let bytes = if needs_raw_bytes {
            match self.raw_bytes_at_target(frame, frame_size, target) {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.report_frame_failure(error);
                    return Ok(());
                }
            }
        } else {
            Vec::new()
        };

        // Debug evidence for the raw-buffer path, written before the writer
        // thread sees the bytes so a frame the encoder rejects still leaves a
        // dump. While recording every frame goes through that path (the
        // GPU-surface path cannot cross to the writer thread), so the dumps
        // cover the first recorded frames whatever the captured size.
        if recording && self.dump_remaining > 0 {
            self.dump_debug_frame(&bytes);
        }

        // Feed: live frames go to the writer queue with a non-blocking
        // `try_send`, everything else goes to the pre-roll ring. The borrow of
        // the writer handle ends with this expression, so the reporting below
        // can use `&mut self` again.
        let push = if let Some(writer) = self.writer.as_ref() {
            Some(writer.live_tx.try_send((timestamp, bytes)))
        } else {
            // No recording: the queue did not consume the bytes, so the ring
            // takes ownership without a second copy.
            if ring_wanted {
                self.ring.push(timestamp, bytes);
            }
            None
        };

        match push {
            Some(Ok(())) => {
                // Ack once, from the capture thread: the recording is live from
                // the first frame that reached the writer's queue. Taking the
                // path is what makes the ack exactly-once.
                if let Some(path) = self.recording_path.take() {
                    self.report(EngineEvent::RecordingStarted { path });
                }
            }
            Some(Err(TrySendError::Full(_dropped))) => {
                // Back-pressure is honest frame loss: the queue holds
                // megabytes and the callback never waits for the encoder.
                tracing::warn!(
                    queue_depth = LIVE_QUEUE_FRAMES,
                    "encoder writer is lagging; live frame dropped"
                );
            }
            Some(Err(TrySendError::Disconnected(_gone))) => {
                // The queue's receiving half is gone: the writer thread ended
                // without leaving a done notice (a panic, or a closed done
                // channel). `is_finished` says so directly, which distinguishes
                // a dead thread from a queue closed while the writer is still
                // muxing. The recording is over and no ack will ever arrive;
                // capture keeps running for the ring.
                let writer_finished = self
                    .writer
                    .as_ref()
                    .is_some_and(|writer| writer.thread.is_finished());
                self.detach_writer();
                tracing::error!(
                    writer_finished,
                    "encoder writer thread ended; recording stopped"
                );
                self.report_command_failure(CaptureError::Encoder(
                    "encoder writer thread ended".to_string(),
                ));
                return Ok(());
            }
            // No recording, so nothing to ack.
            None => {}
        }

        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        tracing::info!("captured monitor closed");
        // The writer thread flushes and muxes on its own; a capture close must
        // not block on encoder work, so only the handle is dropped here.
        self.detach_writer();
        Ok(())
    }
}

#[cfg(windows)]
impl CaptureEngine {
    /// BGRA bytes of the frame at the encoder size, padding stripped and
    /// flipped to the bottom-to-top row order the raw buffer path expects.
    fn raw_bytes_at_target(
        &mut self,
        frame: &mut Frame,
        frame_size: (u32, u32),
        target: (u32, u32),
    ) -> Result<Vec<u8>, CaptureError> {
        let filter = self.config.scale_filter;
        let buffer = frame
            .buffer()
            .map_err(|error| CaptureError::Encoder(format!("frame buffer failed: {error}")))?;

        let unpadded = buffer.as_nopadding_buffer(&mut self.scratch);

        if frame_size == target {
            // Staging rows are top-to-bottom; the encoder's buffer path reads
            // bottom-to-top, so feeding them unflipped records the frame upside
            // down. The GPU-surface path (`send_frame`) needs no flip.
            return Self::flip_or_fail(unpadded, frame_size.0, frame_size.1);
        }

        let resized = resize_bgra(
            unpadded,
            frame_size.0,
            frame_size.1,
            target.0,
            target.1,
            filter,
        );
        if resized.is_empty() {
            // Only a buffer/frame-size mismatch produces this; sending empty
            // bytes to the encoder would corrupt the file instead of failing.
            return Err(CaptureError::Encoder(format!(
                "frame resize produced no bytes for {}x{} -> {}x{}",
                frame_size.0, frame_size.1, target.0, target.1
            )));
        }

        Self::flip_or_fail(&resized, target.0, target.1)
    }

    /// Flip with the same failure contract as the resize path: an empty result
    /// means the buffer and the size disagreed, which must be reported instead
    /// of sending empty bytes to the encoder.
    fn flip_or_fail(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, CaptureError> {
        let flipped = flip_rows_bgra(bytes, width, height);

        if flipped.is_empty() {
            return Err(CaptureError::Encoder(format!(
                "frame flip produced no bytes for {}x{}",
                width, height
            )));
        }

        Ok(flipped)
    }

    /// Write the three debug PNGs for one raw-buffer frame and spend one dump.
    ///
    /// `bytes` are exactly what `send_frame_buffer` is about to receive at
    /// `target` size. Every failure is a warning and a dropped file: a dump is
    /// diagnostic output, so it must never fail the capture.
    ///
    /// `_raw` and `_rgba` hold the identical bytes — their difference is how the
    /// viewer is supposed to read them: `_raw` is the evidence of what was fed
    /// (bottom-to-top, so it renders upside down in an image viewer), `_rgba` is
    /// the same data labelled RGBA, so if its colors look correct the encoder did
    /// *not* swap R/B on its own. `_flipped` is the upright screen — the same
    /// rows reversed, which is how the recording should look.
    fn dump_debug_frame(&mut self, bytes: &[u8]) {
        let Some(dir) = self.dump_dir.clone() else {
            return;
        };

        let index = DEBUG_DUMP_FRAMES - self.dump_remaining;
        self.dump_remaining -= 1;

        let (width, height) = self.target;
        let flipped = flip_rows_bgra(bytes, width, height);

        let dumps = [
            (dir.join(format!("frame_{index}_raw.png")), bytes),
            (
                dir.join(format!("frame_{index}_flipped.png")),
                flipped.as_slice(),
            ),
            (dir.join(format!("frame_{index}_rgba.png")), bytes),
        ];

        for (path, data) in dumps {
            match write_dump_png(&path, width, height, data) {
                Ok(()) => {
                    tracing::info!(path = %path.display(), "debug frame dump written");
                }
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "debug frame dump failed");
                }
            }
        }
    }

    /// Create-on-start recording: the encoder is built **on the capture
    /// thread** (WinRT is initialized there, so `VideoEncoder::new` cannot run
    /// on another thread without `unsafe`, and its ~1 s blocking cost has to
    /// land inside one callback), then moved into a dedicated writer thread
    /// together with the pre-roll snapshot.
    ///
    /// Nothing here waits for the encoder. The ack (`RecordingStarted`) is
    /// reported later — by the first live frame that reaches the writer's
    /// queue — and a replay/send failure arrives as `CommandFailed` from the
    /// writer thread, so a slow or dead encoder never stalls this callback.
    fn start_recording(&mut self, path: PathBuf) {
        if let Err(error) = validate_output_path(&path) {
            tracing::error!(path = %path.display(), %error, "recording start rejected");
            self.report_command_failure(error);
            return;
        }

        if self.writer.is_some() {
            tracing::warn!(path = %path.display(), "recording already running");
            self.report_command_failure(CaptureError::Encoder(
                "a recording is already running".to_string(),
            ));
            return;
        }

        let encoder = match self.create_encoder(&path) {
            Ok(encoder) => encoder,
            Err(error) => {
                tracing::error!(path = %path.display(), %error, "encoder creation failed");
                self.report_command_failure(error);
                return;
            }
        };

        // Pre-roll snapshot, filtered here on the capture thread: the ring
        // already owns the byte buffers, so handing them to the writer thread
        // is a move, not a copy.
        let target_bytes = frame_bytes(self.target.0, self.target.1);
        let replay = self.ring.take();
        let buffered = replay.len();
        let pre_roll: Vec<(i64, Vec<u8>)> = replay
            .into_iter()
            .filter(|(_, frame)| frame.len() == target_bytes)
            .collect();
        let replayed = pre_roll.len();
        let skipped = buffered - replayed;

        tracing::info!(
            path = %path.display(),
            replayed,
            skipped,
            target = ?self.target,
            "recording started"
        );

        // Bounded live queue (frames are megabytes, so the depth is a RAM
        // budget) and the writer thread that owns the encoder. The sending
        // halves are cloned once each to move them into the thread; a
        // `Sender`/`SyncSender` clone is O(1), and the engine keeps its own
        // copies so both channels stay open between recordings.
        let (live_tx, live_rx) = mpsc::sync_channel(LIVE_QUEUE_FRAMES);
        let generation = self.writer_generation + 1;
        self.writer_generation = generation;

        // One clone per owner: the ack needs an owned path on the event channel
        // and the writer thread needs its own copy for logging.
        let ack_path = path.clone();
        let done_tx = self.done_tx.clone();
        let event_tx = self.event_tx.clone();

        let thread = std::thread::spawn(move || {
            run_encoder_writer(
                encoder, path, generation, pre_roll, live_rx, done_tx, event_tx,
            )
        });

        self.writer = Some(EncoderWriter { live_tx, thread });
        self.recording_path = Some(ack_path);

        // Clear the pacing clock where recording begins: after a Stop→Start
        // inside one frame interval the stale baseline would drop the first
        // frame of the new recording; with the clock cleared, the first-frame
        // rule in `should_send` accepts the first frame fed to the writer.
        self.last_timestamp = None;
    }

    fn create_encoder(&self, path: &Path) -> Result<VideoEncoder, CaptureError> {
        let sub_type = match self.config.codec {
            Codec::H264 => VideoSettingsSubType::H264,
            Codec::Hevc => VideoSettingsSubType::HEVC,
        };

        let video = VideoSettingsBuilder::new(self.target.0, self.target.1)
            .sub_type(sub_type)
            .bitrate(self.config.bitrate_bps())
            .frame_rate(u32::from(self.config.fps));

        VideoEncoder::new(
            video,
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            path,
        )
        .map_err(|error| CaptureError::Encoder(format!("encoder creation failed: {error}")))
    }

    /// Ask the writer thread to finish the current recording: dropping the
    /// handle closes `live_tx`, so the writer drains whatever is already queued,
    /// muxes the file, and reports `RecordingStopped` (or `CommandFailed`) on
    /// the event channel itself.
    ///
    /// This never blocks — `finish()` would wait for the transcode thread, and a
    /// frame callback must not. The session release is armed by the writer's
    /// done notice, which always arrives after its ack, so the facade still sees
    /// `RecordingStopped` before `CaptureReleased`.
    fn stop_recording(&mut self) {
        if self.writer.is_none() {
            tracing::warn!("stop requested while no recording is running");
            self.report_command_failure(CaptureError::NotConfigured);
            return;
        }

        self.detach_writer();
        tracing::info!("recording stop requested; writer thread is finishing the file");
    }

    /// Drop the writer handle without waiting for the writer thread. Closing
    /// `live_tx` is its "no more live frames" signal; the engine never joins it
    /// (a frame callback must not block on muxing), so dropping the
    /// `JoinHandle` detaches the thread and it runs to the end on its own.
    fn detach_writer(&mut self) {
        self.recording_path = None;
        self.writer = None;
    }

    /// Apply a runtime replay-window change to the ring on the capture thread.
    ///
    /// The requested seconds are clamped by the configuration's own rule
    /// ([`CaptureConfig::replay_seconds`]), so a runtime request obeys the same
    /// `0..MAX_REPLAY_SECONDS` bound as the startup config; `self.config` stays
    /// untouched (it is what the facade reports). The capacity is
    /// [`ring_capacity`] of that clamped value, i.e. seconds × fps.
    ///
    /// Fire-and-forget: no event is sent, so the sender never waits and the
    /// frame path stays non-blocking. `0` drains the ring and releases its
    /// buffers; a larger value keeps the frames already buffered up to the new
    /// window. While a recording is active the ring is not fed, so enabling
    /// during a recording fills the ring only after recording stops.
    fn set_replay(&mut self, seconds: u32) {
        // Struct update reuses the config clamp with only `replay_seconds`
        // replaced, so no second copy of the clamping rule lives here.
        let requested = CaptureConfig {
            replay_seconds: seconds,
            ..self.config
        };
        let clamped = requested.replay_seconds();
        let frames = ring_capacity(&requested);

        self.ring.set_capacity(frames);
        self.ring_capacity = frames;

        if frames == 0 {
            // Buffer disabled: with no recording in progress nothing will ever
            // be buffered again, so the next frame releases the session (spec
            // v4 item 2). A running recording keeps the session open — its done
            // notice arms the release when the file is muxed.
            self.release_session = session_should_release(frames, self.writer.is_some());
            if self.release_session {
                tracing::info!("replay buffer disabled while idle; capture session released");
            }

            tracing::info!(requested = seconds, "replay buffer disabled (ring drained)");
            return;
        }

        // The ring stores frames at the encoder target size, so this is the
        // real upper bound of the buffered BGRA bytes for the new window.
        tracing::info!(
            requested = seconds,
            seconds = clamped,
            frames,
            max_bytes = estimated_memory_bytes(
                clamped,
                u32::from(self.config.fps),
                self.target.0,
                self.target.1
            ),
            "replay buffer enabled",
        );
    }

    fn report(&mut self, event: EngineEvent) {
        if self.event_tx.send(event).is_err() {
            tracing::warn!("event channel closed; report dropped");
        }
    }

    fn report_command_failure(&mut self, error: CaptureError) {
        self.report(EngineEvent::CommandFailed { error });
    }

    fn report_frame_failure(&mut self, error: CaptureError) {
        self.report(EngineEvent::FrameFailed { error });
    }
}

/// Public entry point: owns the capture thread and its command channels.
///
/// On Windows, [`Recorder::new`] starts the capture thread immediately (the
/// replay ring fills from then on), and [`Recorder::recording`] starts it with a
/// recording already queued. `start`/`stop` drive the recording by **queueing**
/// commands (never blocking), `poll_recording_ack`/`poll_stop_ack` report the
/// outcome of those queued commands, `enable_replay`/`disable_replay` change the
/// pre-roll window at runtime, and `shutdown` stops capture and joins the
/// thread, returning whatever the thread reported. On Linux every capture method
/// returns [`CaptureError::UnsupportedPlatform`] and no thread is started; the
/// replay toggle still reports the requested state through `replay_enabled`, and
/// both poll methods report `None` because nothing can be queued.
///
/// When the capture thread reports [`EngineEvent::CaptureReleased`] (a recording
/// stopped while the replay ring had no capacity), the facade records the
/// release while polling acks and refuses further `start`/`stop`/`enable_replay`
/// /`disable_replay` commands with [`CaptureError::NotConfigured`];
/// `replay_enabled` keeps reporting the requested state.
///
/// Deliberately **not** `Clone`: the recorder owns a single capture thread and
/// one command channel, and its methods take `&mut self` so exactly one caller
/// drives the session.
pub struct Recorder {
    config: CaptureConfig,
    /// Replay state as *requested* by the facade, kept locally so
    /// [`Recorder::replay_enabled`] answers without asking the capture thread.
    /// Seeded from the startup config (`replay_seconds > 0`) and updated by
    /// [`Recorder::enable_replay`]/[`Recorder::disable_replay`].
    replay_enabled: bool,
    /// Pending ack of the queued `Start` command, consumed by
    /// [`Recorder::poll_recording_ack`]. `Idle` on Linux (nothing is queued).
    pending_start: AckState,
    /// Pending ack of the queued `Stop` command, consumed by
    /// [`Recorder::poll_stop_ack`]. `Idle` on Linux (nothing is queued).
    pending_stop: AckState,
    /// Set when the capture thread reports [`EngineEvent::CaptureReleased`]
    /// (the recording stopped while the replay ring had no capacity, so
    /// `capture_control.stop()` ended the capture session). A released
    /// session can no longer accept commands: `start`, `stop`,
    /// `enable_replay`, and `disable_replay` report
    /// [`CaptureError::NotConfigured`], the same error `shutdown` reports for
    /// a session that is gone.
    session_released: bool,
    #[cfg(windows)]
    commands: Option<mpsc::Sender<EngineCommand>>,
    #[cfg(windows)]
    events: Option<mpsc::Receiver<EngineEvent>>,
    #[cfg(windows)]
    control: Option<CaptureControl<CaptureEngine, CaptureError>>,
}

/// Fold the non-blocking drain of the event channel into the pending state of
/// one command.
///
/// The matching ack marks `Acked`, a `CommandFailed` marks `Failed`; anything
/// else — an asynchronous `FrameFailed` or the other command's ack — is logged
/// and left in the channel's wake-up order for the next poll or `shutdown`, so
/// no event is swallowed silently. Draining stops at the first outcome for this
/// command.
///
/// `released` records a [`EngineEvent::CaptureReleased`]: the session is gone,
/// so the facade must refuse later commands. The release is **not** an outcome
/// for the command being polled, so it does not consume the pending ack — the
/// drain records it and keeps going until the command's own event arrives.
#[cfg(windows)]
fn drain_events(
    receiver: &mut mpsc::Receiver<EngineEvent>,
    command: &str,
    acked: fn(&EngineEvent) -> bool,
    pending: &mut AckState,
    released: &mut bool,
) {
    while let Ok(event) = receiver.try_recv() {
        if acked(&event) {
            *pending = AckState::Acked;
            return;
        }

        if event_is_capture_release(&event) {
            *released = true;
            continue;
        }

        match event {
            EngineEvent::CommandFailed { error } => {
                tracing::error!(command, %error, "capture command failed");
                *pending = AckState::Failed(error);
                return;
            }
            EngineEvent::FrameFailed { error } => {
                tracing::warn!(command, %error, "frame dropped while an ack is pending");
            }
            other => {
                tracing::warn!(command, ?other, "unrelated event while an ack is pending");
            }
        }
    }
}

/// Drain the event channel looking only for the session-release signal.
///
/// Used when no command ack is pending (see [`Recorder::poll_session_release`]):
/// a `CaptureReleased` sets `released` and the drain continues; every other
/// event is logged, never turned into an ack, so an ack that arrives with no
/// command queued shows up as a stray event instead of silently satisfying
/// some other state.
#[cfg(windows)]
fn drain_release(receiver: &mut mpsc::Receiver<EngineEvent>, released: &mut bool) {
    while let Ok(event) = receiver.try_recv() {
        if event_is_capture_release(&event) {
            *released = true;
            continue;
        }

        tracing::warn!(?event, "unrelated event while no command is pending");
    }
}

/// One non-blocking ack poll: drain the channel (if it still exists), then let
/// [`ack_outcome`] decide what to report at `now`.
///
/// A channel that `shutdown` already took can never deliver an ack, so a
/// command still pending at that point is reported as
/// [`CaptureError::NotConfigured`] rather than waiting for its deadline.
#[cfg(windows)]
fn poll_pending(
    events: &mut Option<mpsc::Receiver<EngineEvent>>,
    command: &str,
    acked: fn(&EngineEvent) -> bool,
    pending: &mut AckState,
    released: &mut bool,
    now: Instant,
) -> Option<Result<(), CaptureError>> {
    let Some(receiver) = events.as_mut() else {
        if matches!(pending, AckState::Pending { .. }) {
            tracing::error!(command, "event channel is gone; the ack can never arrive");
            *pending = AckState::Failed(CaptureError::NotConfigured);
        }
        return ack_outcome(command, pending, now);
    };

    drain_events(receiver, command, acked, pending, released);

    ack_outcome(command, pending, now)
}

/// Which ack event belongs to the queued `Start` command.
#[cfg(windows)]
fn recording_started_ack(event: &EngineEvent) -> bool {
    matches!(event, EngineEvent::RecordingStarted { .. })
}

/// Which ack event belongs to the queued `Stop` command.
#[cfg(windows)]
fn recording_stopped_ack(event: &EngineEvent) -> bool {
    matches!(event, EngineEvent::RecordingStopped)
}

impl Recorder {
    /// The configuration this recorder was created with.
    #[must_use]
    pub const fn config(&self) -> CaptureConfig {
        self.config
    }

    /// Start the always-on capture thread and return the recorder.
    ///
    /// Fails when the capture pipeline cannot start (no primary monitor, WinRT
    /// or Direct3D init failure) — the failure is traced and returned, never
    /// panicking. An unusable configuration (`fps == 0` or
    /// `Resolution::Height(0)`) is refused first, with
    /// [`CaptureError::NotConfigured`], before any thread is started.
    ///
    /// Honors `VC_DEBUG_DUMP_DIR` as the debug dump directory (see
    /// [`Recorder::with_debug_dump`]); an unset variable means no dumps.
    #[cfg(windows)]
    pub fn new(config: CaptureConfig) -> Result<Self, CaptureError> {
        Self::start_capture(config, None, None)
    }

    /// [`Recorder::new`] with a debug frame dump directory.
    ///
    /// When a directory is given (`Some`, or via `VC_DEBUG_DUMP_DIR` when the
    /// argument is `None`), the first three frames of a recording are written
    /// there as `frame_<index>_raw.png`, `frame_<index>_flipped.png`, and
    /// `frame_<index>_rgba.png` before they are queued to the encoder writer
    /// thread — every recorded frame goes through the raw-buffer path, so the
    /// dumps cover recorded frames whatever the captured size. Dump failures
    /// are logged as warnings and never fail the capture. Refuses an unusable
    /// configuration like [`Recorder::new`].
    #[cfg(windows)]
    pub fn with_debug_dump(
        config: CaptureConfig,
        dump_dir: Option<PathBuf>,
    ) -> Result<Self, CaptureError> {
        Self::start_capture(config, dump_dir, None)
    }

    /// [`Recorder::new`] with a recording already queued, so the **first
    /// captured frame** starts the recording.
    ///
    /// `path` is validated first ([`CaptureError::InvalidPath`]), then the
    /// configuration ([`CaptureError::NotConfigured`], see [`Recorder::new`]),
    /// the command
    /// channel is built and `EngineCommand::Start(path)` is enqueued into it
    /// *before* `start_free_threaded`, and the pending ack deadline is marked —
    /// so the recording starts on the frame the thread delivers first, instead
    /// of waiting for a `start` call to land between frames. Like
    /// [`Recorder::start`], this returns `Ok(())` once queued and never waits
    /// for the ack: recording actually begins when the capture thread has
    /// created the encoder and handed it to its writer thread, and the outcome
    /// is reported by
    /// [`Recorder::poll_recording_ack`] (`Some(Ok(()))` on the ack,
    /// `Some(Err(..))` on a failure event or an elapsed deadline, `None` while
    /// still pending).
    ///
    /// Fails like [`Recorder::new`] when the capture pipeline cannot start. A
    /// path that validation rejects, or a command the channel cannot accept, is
    /// reported *before* any capture thread is started, so a failed boot never
    /// leaves a thread running.
    #[cfg(windows)]
    pub fn recording(
        config: CaptureConfig,
        path: impl Into<PathBuf>,
    ) -> Result<Self, CaptureError> {
        let path = path.into();
        validate_output_path(&path)?;

        Self::start_capture(config, None, Some(path))
    }

    /// Shared Windows startup: the argument wins over the environment variable.
    /// `boot_start` queues a recording before the capture thread is started.
    #[cfg(windows)]
    fn start_capture(
        config: CaptureConfig,
        dump_dir: Option<PathBuf>,
        boot_start: Option<PathBuf>,
    ) -> Result<Self, CaptureError> {
        reject_invalid_config(&config)?;

        let dump_dir = dump_dir.or_else(debug_dump_dir_from_env);
        // The startup config decides the initial replay state before `config` is
        // moved into the capture flags.
        let replay_enabled = config.replay_seconds() > 0;

        tracing::info!(
            parameters = %config_dump(&config),
            dump_dir = ?dump_dir,
            "recorder starting"
        );

        let monitor = Monitor::primary().map_err(|error| {
            CaptureError::Encoder(format!("primary monitor unavailable: {error}"))
        })?;

        let native = (
            monitor
                .width()
                .map_err(|error| CaptureError::Encoder(format!("monitor width failed: {error}")))?,
            monitor.height().map_err(|error| {
                CaptureError::Encoder(format!("monitor height failed: {error}"))
            })?,
        );

        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        // The boot command is queued *before* `start_free_threaded`: the message
        // waits in the channel until the handler's first `try_recv`, so the
        // recording activates on the first captured frame instead of on a later
        // one. A rejected queue fails here, so no capture thread is started.
        let mut pending_start = AckState::Idle;

        if let Some(path) = boot_start {
            if let Err(error) = command_tx.send(EngineCommand::Start(path)) {
                tracing::error!(error = ?error, "boot recording could not be queued");
                return Err(CaptureError::Encoder(
                    "command channel closed before the boot start was queued".to_string(),
                ));
            }

            pending_start = AckState::Pending {
                deadline: Instant::now() + ACK_TIMEOUT,
            };
        }

        let (minimum_update_interval, interval_mode) = minimum_update_interval(&config);

        let settings = Settings::new(
            monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            // A hint only: the API does not guarantee a constant rate, so the
            // handler paces itself on top of this. `Default` is used when the
            // platform has no `MinUpdateInterval` property (see
            // `minimum_update_interval`).
            minimum_update_interval,
            DirtyRegionSettings::Default,
            // BGRA is what `send_frame_buffer` expects, and it matches the
            // encoder's uncompressed surface format, so no channel swap happens.
            ColorFormat::Bgra8,
            EngineFlags {
                config,
                native,
                interval_mode,
                dump_dir,
                command_rx,
                event_tx,
            },
        );

        let control = CaptureEngine::start_free_threaded(settings).map_err(|error| {
            CaptureError::Encoder(format!("capture thread failed to start: {error}"))
        })?;

        Ok(Self {
            config,
            replay_enabled,
            pending_start,
            pending_stop: AckState::Idle,
            session_released: false,
            commands: Some(command_tx),
            events: Some(event_rx),
            control: Some(control),
        })
    }

    /// Linux: capture is Windows-only, so nothing is started. An unusable
    /// configuration is still refused with [`CaptureError::NotConfigured`] —
    /// configuration validity is platform-independent.
    #[cfg(not(windows))]
    #[allow(clippy::unnecessary_wraps)] // same signature as the Windows build
    pub fn new(config: CaptureConfig) -> Result<Self, CaptureError> {
        reject_invalid_config(&config)?;

        tracing::info!(parameters = %config_dump(&config), "recorder starting");

        Ok(Self {
            config,
            replay_enabled: config.replay_seconds() > 0,
            pending_start: AckState::Idle,
            pending_stop: AckState::Idle,
            session_released: false,
        })
    }

    /// Linux: the debug dumps belong to the Windows raw-buffer path, so no
    /// recorder with a dump directory can be built; this reports the same
    /// platform failure the capture methods report. An unusable configuration
    /// is refused with [`CaptureError::NotConfigured`] first, like
    /// [`Recorder::new`].
    #[cfg(not(windows))]
    pub fn with_debug_dump(
        config: CaptureConfig,
        _dump_dir: Option<PathBuf>,
    ) -> Result<Self, CaptureError> {
        reject_invalid_config(&config)?;

        Err(CaptureError::UnsupportedPlatform)
    }

    /// Linux: no capture thread can be started, so a boot recording is not
    /// possible either — the same [`CaptureError::UnsupportedPlatform`] the
    /// other capture stubs report, and no path validation is consulted. An
    /// unusable configuration is refused with [`CaptureError::NotConfigured`]
    /// first, like [`Recorder::new`].
    #[cfg(not(windows))]
    pub fn recording(
        config: CaptureConfig,
        _path: impl Into<PathBuf>,
    ) -> Result<Self, CaptureError> {
        reject_invalid_config(&config)?;

        Err(CaptureError::UnsupportedPlatform)
    }

    /// Queue a recording to `path`; the encoder is created on the capture
    /// thread, so recording actually begins once that thread has activated it.
    ///
    /// Returns `Ok(())` as soon as the command is queued — it never waits for
    /// the ack (the ack arrives on the next captured frame, and encoder
    /// activation takes ~1 s). Failure modes are the ones this API has always
    /// had: [`CaptureError::InvalidPath`] for a rejected path (checked here,
    /// before queueing), [`CaptureError::NotConfigured`] once `shutdown` has
    /// taken the channel **or** once the capture thread has released the
    /// session (`EngineEvent::CaptureReleased` — checked first, before path
    /// validation), [`CaptureError::Encoder`] when the send finds the channel
    /// closed. The capture thread's own outcome — encoder creation
    /// failure, "a recording is already running", or an elapsed deadline — is
    /// reported by [`Recorder::poll_recording_ack`].
    #[cfg(windows)]
    pub fn start(&mut self, path: impl Into<PathBuf>) -> Result<(), CaptureError> {
        if self.session_released {
            tracing::error!("capture session released; start refused");
            return Err(CaptureError::NotConfigured);
        }

        let path = path.into();
        validate_output_path(&path)?;

        let sender = self.commands.as_ref().ok_or(CaptureError::NotConfigured)?;
        sender
            .send(EngineCommand::Start(path))
            .map_err(|error| CaptureError::Encoder(format!("command channel closed: {error}")))?;

        self.pending_start = AckState::Pending {
            deadline: Instant::now() + ACK_TIMEOUT,
        };

        Ok(())
    }

    /// Linux: no capture thread exists, so a recording cannot start and no ack
    /// is ever pending. A released session is refused with
    /// [`CaptureError::NotConfigured`] first — the same refusal the Windows
    /// facade reports once the capture thread released the session.
    #[cfg(not(windows))]
    #[allow(clippy::unnecessary_wraps)] // same signature as the Windows build
    pub fn start(&mut self, _path: impl Into<PathBuf>) -> Result<(), CaptureError> {
        if self.session_released {
            return Err(CaptureError::NotConfigured);
        }

        Err(CaptureError::UnsupportedPlatform)
    }

    /// Queue the end of the current recording; the capture keeps running.
    ///
    /// Returns `Ok(())` as soon as the command is queued — it never waits for
    /// the ack (muxing the file takes seconds). Same failure modes as
    /// [`Recorder::start`] minus path validation; "no recording is running" and
    /// a failed `finish()` arrive as events, reported by
    /// [`Recorder::poll_stop_ack`]. A session released after an earlier stop
    /// (with the replay ring disabled) is refused with
    /// [`CaptureError::NotConfigured`] before anything else.
    #[cfg(windows)]
    pub fn stop(&mut self) -> Result<(), CaptureError> {
        if self.session_released {
            tracing::error!("capture session released; stop refused");
            return Err(CaptureError::NotConfigured);
        }

        let sender = self.commands.as_ref().ok_or(CaptureError::NotConfigured)?;
        sender
            .send(EngineCommand::Stop)
            .map_err(|error| CaptureError::Encoder(format!("command channel closed: {error}")))?;

        self.pending_stop = AckState::Pending {
            deadline: Instant::now() + ACK_TIMEOUT,
        };

        Ok(())
    }

    /// Linux: no capture thread exists, so there is nothing to stop. A
    /// released session is refused with [`CaptureError::NotConfigured`] first,
    /// mirroring the Windows refusal.
    #[cfg(not(windows))]
    #[allow(clippy::unnecessary_wraps)] // same signature as the Windows build
    pub fn stop(&mut self) -> Result<(), CaptureError> {
        if self.session_released {
            return Err(CaptureError::NotConfigured);
        }

        Err(CaptureError::UnsupportedPlatform)
    }

    /// Non-blocking read of the outcome of the queued `Start` command.
    ///
    /// Drains the event channel with `try_recv`: `Some(Ok(()))` once
    /// `EngineEvent::RecordingStarted` arrives, `Some(Err(..))` when a
    /// `CommandFailed` arrives or the pending deadline (`ACK_TIMEOUT`) elapses,
    /// `None` while still pending. The outcome is reported once — a later poll
    /// of the same command returns `None` — and unrelated events
    /// (`FrameFailed`, the other command's ack) are logged, never swallowed.
    /// A `CaptureReleased` is not an ack for either command: it marks
    /// [`Recorder::session_released`] and the drain continues, so the ack of a
    /// command queued before the release is still awaited (and times out).
    /// Call it from the UI loop (or after a frame has been delivered) to learn
    /// that the recording is live.
    #[cfg(windows)]
    pub fn poll_recording_ack(&mut self) -> Option<Result<(), CaptureError>> {
        poll_pending(
            &mut self.events,
            "start",
            recording_started_ack,
            &mut self.pending_start,
            &mut self.session_released,
            Instant::now(),
        )
    }

    /// Non-blocking read of the outcome of the queued `Stop` command: `Some(Ok(()))`
    /// once the file is muxed and closed (`EngineEvent::RecordingStopped`),
    /// `Some(Err(..))` on a failure event or an elapsed deadline, `None` while
    /// still pending. Reported once; unrelated events are logged. A
    /// `CaptureReleased` — the usual event right after a stop whose ring had no
    /// capacity — marks [`Recorder::session_released`] without consuming the
    /// stop ack.
    #[cfg(windows)]
    pub fn poll_stop_ack(&mut self) -> Option<Result<(), CaptureError>> {
        poll_pending(
            &mut self.events,
            "stop",
            recording_stopped_ack,
            &mut self.pending_stop,
            &mut self.session_released,
            Instant::now(),
        )
    }

    /// Non-blocking read of the session-release signal when **no command is
    /// queued**: switching the replay buffer off with no recording releases the
    /// session on the next frame, and that release is not an ack for any
    /// command, so there is no ack poll to carry it. While a `Start` or `Stop`
    /// ack is pending this reads nothing (draining would swallow that ack) and
    /// just returns the flag already recorded by [`Recorder::poll_recording_ack`]
    /// / [`Recorder::poll_stop_ack`]. Returns [`Recorder::session_released`]:
    /// once it is `true`, every further command fails with
    /// [`CaptureError::NotConfigured`] and the capture indicator is gone.
    #[cfg(windows)]
    pub fn poll_session_release(&mut self) -> bool {
        if matches!(self.pending_start, AckState::Pending { .. })
            || matches!(self.pending_stop, AckState::Pending { .. })
        {
            return self.session_released;
        }

        if let Some(receiver) = self.events.as_mut() {
            drain_release(receiver, &mut self.session_released);
        }

        self.session_released
    }

    /// Linux: nothing is ever queued, so there is never an ack outcome to
    /// report — the stub convention for both poll methods is `None`, reached
    /// through the same [`ack_outcome`] decision as on Windows (the pending
    /// state stays `Idle`).
    #[cfg(not(windows))]
    pub fn poll_recording_ack(&mut self) -> Option<Result<(), CaptureError>> {
        ack_outcome("start", &mut self.pending_start, Instant::now())
    }

    /// Linux: same stub convention as [`Recorder::poll_recording_ack`] — `None`,
    /// because `stop` never queues a command here.
    #[cfg(not(windows))]
    pub fn poll_stop_ack(&mut self) -> Option<Result<(), CaptureError>> {
        ack_outcome("stop", &mut self.pending_stop, Instant::now())
    }

    /// Linux: capture is Windows-only, so no release event can ever arrive;
    /// the stub reports the flag, which stays `false`.
    #[cfg(not(windows))]
    #[must_use]
    pub fn poll_session_release(&self) -> bool {
        self.session_released
    }

    /// Request the runtime pre-roll replay window of `seconds` (`0` disables it).
    ///
    /// Fire-and-forget: the command is put on the existing command channel and
    /// the capture thread never acks it, so this never waits. The window change
    /// itself takes effect on the **next captured frame**; the facade's own
    /// [`Recorder::replay_enabled`] reflects the request immediately, without
    /// any roundtrip. Because the ring is not fed while a recording is active,
    /// enabling during an active recording fills the ring only after recording
    /// stops. Seconds are clamped to `0..MAX_REPLAY_SECONDS` by the engine, the
    /// same bound as the startup config.
    ///
    /// Fails when the capture thread is gone: [`CaptureError::NotConfigured`]
    /// once the capture thread has released the session
    /// (`EngineEvent::CaptureReleased`) or once `shutdown` has taken the
    /// channel, [`CaptureError::Encoder`] when the send itself finds the
    /// channel closed. A successful send returns `Ok(())`.
    #[cfg(windows)]
    pub fn enable_replay(&mut self, seconds: u32) -> Result<(), CaptureError> {
        if self.session_released {
            tracing::error!("capture session released; replay request refused");
            return Err(CaptureError::NotConfigured);
        }

        let sender = self.commands.as_ref().ok_or(CaptureError::NotConfigured)?;
        sender
            .send(EngineCommand::SetReplay(seconds))
            .map_err(|error| CaptureError::Encoder(format!("command channel closed: {error}")))?;

        // Only a delivered request changes the reported state.
        self.replay_enabled = seconds > 0;

        Ok(())
    }

    /// Linux: no capture thread and no ring exist, so the request cannot be
    /// applied and reports [`CaptureError::UnsupportedPlatform`] like the other
    /// capture methods. The requested state is still recorded locally, so
    /// [`Recorder::replay_enabled`] keeps reporting what was asked for. A
    /// released session is refused with [`CaptureError::NotConfigured`] first —
    /// before the state is recorded, since nothing can be asked of a gone
    /// session.
    #[cfg(not(windows))]
    #[allow(clippy::unnecessary_wraps)] // same signature as the Windows build
    pub fn enable_replay(&mut self, seconds: u32) -> Result<(), CaptureError> {
        if self.session_released {
            return Err(CaptureError::NotConfigured);
        }

        self.replay_enabled = seconds > 0;

        Err(CaptureError::UnsupportedPlatform)
    }

    /// Disable the pre-roll replay buffer: the same request as
    /// [`Recorder::enable_replay`] with `seconds = 0`, so the ring is drained
    /// and its buffers released on the next captured frame. Same failure modes
    /// as [`Recorder::enable_replay`].
    pub fn disable_replay(&mut self) -> Result<(), CaptureError> {
        self.enable_replay(0)
    }

    /// Whether the replay buffer is currently requested, read from the facade's
    /// own state — no roundtrip to the capture thread.
    ///
    /// Seeded from `config.replay_seconds` (`> 0` means enabled) at
    /// construction and updated by [`Recorder::enable_replay`] /
    /// [`Recorder::disable_replay`]. The applied change lands on the **next
    /// captured frame**, and enabling during an active recording fills the ring
    /// only after recording stops.
    #[must_use]
    pub fn replay_enabled(&self) -> bool {
        self.replay_enabled
    }

    /// Stop the capture thread, join it, and report what it failed on the way
    /// out (an encoder that could not finish, or a thread that ended in error).
    ///
    /// The encoder's writer thread is **not** joined here: the capture thread
    /// only drops its handle, so a file that was still muxing keeps being
    /// written by that detached thread after this returns. `Quit` stops capture
    /// without waiting for it, because a frame callback must never block on
    /// encoder work.
    #[cfg(windows)]
    pub fn shutdown(&mut self) -> Result<(), CaptureError> {
        if let Some(sender) = self.commands.take()
            && sender.send(EngineCommand::Quit).is_err()
        {
            tracing::warn!("command channel closed before quit");
        }

        let control = self.control.take();
        let events = self.events.take();

        let stopped = match control {
            Some(control) => control.stop().map_err(|error| {
                CaptureError::Encoder(format!("capture thread ended with error: {error}"))
            }),
            None => Err(CaptureError::NotConfigured),
        };

        let reported = events.and_then(|receiver| {
            receiver.try_iter().find_map(|event| match event {
                EngineEvent::CommandFailed { error } | EngineEvent::FrameFailed { error } => {
                    Some(error)
                }
                EngineEvent::RecordingStarted { .. } | EngineEvent::RecordingStopped => None,
                // A released session is not a failure; the facade records it
                // as `session_released` while polling acks.
                EngineEvent::CaptureReleased => None,
            })
        });

        stopped?;

        match reported {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Linux: nothing to shut down.
    #[cfg(not(windows))]
    #[allow(clippy::unnecessary_wraps)] // same signature as the Windows build
    pub fn shutdown(&mut self) -> Result<(), CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }
}

/// Drop cleans up like [`Recorder::shutdown`] so a dropped recorder never leaves
/// a capture thread running with an open file.
#[cfg(windows)]
impl Drop for Recorder {
    fn drop(&mut self) {
        if self.control.is_some()
            && let Err(error) = self.shutdown()
        {
            tracing::warn!(%error, "recorder drop cleanup failed");
        }
    }
}

#[cfg(test)]
#[path = "engine.tests.rs"]
mod tests;
