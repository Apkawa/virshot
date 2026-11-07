//! Isolated Windows screen-recording crate over `windows-capture`.
//!
//! The public surface is platform-independent: [`config`] holds the encoding
//! parameters the library can actually honor, [`ring`] is the pre-roll replay
//! buffer, [`error`] carries typed failures, and [`engine`] is the capture loop
//! plus the [`Recorder`] facade.
//!
//! `Recorder` is the entry point: `Recorder::new(config)` starts an always-on
//! capture thread (Windows) that keeps the replay ring warm,
//! `Recorder::recording(config, path)` starts it with a recording already
//! queued so the first captured frame activates the encoder, `start(path)` and
//! `stop()` **queue** the recording commands and return immediately,
//! `poll_recording_ack()`/`poll_stop_ack()` report the outcome of those queued
//! commands, `enable_replay(seconds)`/`disable_replay()` change the pre-roll
//! window at runtime (`replay_enabled()` reads the requested state), and
//! `shutdown()` stops capture and joins the thread.
//!
//! Every constructor checks the configuration first: [`config::CaptureConfig::invalid_reason`]
//! names the values no capture can use (`fps == 0`, `Resolution::Height(0)`) in
//! Russian, and a refused configuration comes back as
//! [`error::CaptureError::NotConfigured`] — never a panic.
//!
//! **Queued commands, polled acks** (Windows): `VideoEncoder::new` activation
//! (~1 s) and `finish()` muxing are work the capture thread does on a frame
//! arrival, so `start`/`stop` never block the caller — they validate the path,
//! put the command on the channel, and record a pending deadline
//! (`engine::ACK_TIMEOUT`). The result is read with `poll_recording_ack` /
//! `poll_stop_ack`, which drain the event channel with `try_recv`:
//! `Some(Ok(()))` once the matching ack arrives, `Some(Err(..))` when a
//! `CommandFailed` arrives or the deadline elapses, `None` while still pending.
//! Each outcome is reported once; asynchronous `FrameFailed` events and the
//! other command's acks are logged, not swallowed. A UI loop calls `start`,
//! keeps painting, and polls the ack to flip its state.
//!
//! Capture itself is Windows-only; on Linux the crate compiles and every
//! capture path returns [`CaptureError::UnsupportedPlatform`]
//! (error::CaptureError), and both poll methods return `None` because nothing
//! can be queued.
//!
//! **Debug frame dumps** (Windows): `Recorder::with_debug_dump(config,
//! Some(dir))` — or plain `Recorder::new(config)` with the `VC_DEBUG_DUMP_DIR`
//! environment variable set, where the constructor argument wins over the
//! variable — writes the first three frames that go through the raw-buffer path
//! (captured frame size != encoder target size) into `dir` as
//! `frame_<index>_raw.png`, `frame_<index>_flipped.png`, and
//! `frame_<index>_rgba.png`. `_raw` and `_rgba` are the exact bytes fed to
//! `send_frame_buffer` encoded as RGBA PNGs (no channel swap), `_flipped` is
//! [`engine::flip_rows_bgra`] of them, so the row order, stride, and channel
//! interpretation Media Foundation applies can be checked empirically. Dump
//! failures are warnings only and never fail the capture; on Linux
//! `with_debug_dump` returns `UnsupportedPlatform`.
//!
//! **Row orientation** (Windows): a captured frame's CPU buffer is
//! top-to-bottom, while Media Foundation's raw-buffer path renders
//! top-to-bottom samples upside down. The engine therefore flips the rows
//! ([`engine::flip_rows_bgra`]) before `send_frame_buffer`, and the replay ring
//! stores those same flipped bytes. Frames whose size matches the encoder
//! target go through the GPU surface (`send_frame`) and are never flipped. In
//! a dump, `_raw` looks upside down and `_flipped` looks like the screen.
//!
//! **Scaling** (Windows): a resolution fixes the frame **height** — any
//! [`config::Resolution::Height`] in rows, or native via
//! [`config::Resolution::Full`] — and derives the width from the desktop's
//! aspect ratio with one uniform scale ([`config::Resolution::target_size`]);
//! the desktop is never upscaled, and a desktop shorter than the requested
//! height keeps its native size. When
//! the captured frame size differs from that target, every frame is resized on
//! the CPU ([`engine::resize_bgra`]) with the configured
//! [`config::ScaleFilter`]: `Nearest` and `Bilinear` (the default) run through
//! this crate's own kernels, `CatmullRom`, `Gaussian`, and
//! `Lanczos3` through `image::imageops::resize`. The `image` filters cost
//! 150–210 ms per frame at 1680x1050 -> 1152x720, which is far below real-time,
//! so they are quality options with an honest cost label. A frame whose size
//! already matches the target goes through the GPU surface and is never resized,
//! so `ScaleFilter` has no effect there (notably on `Resolution::Full`).
//!
//! **Not implemented**, and why (honest list):
//! - rate control beyond CBR — the library exposes one target bitrate
//!   (`bitrate_kbps`, applied as `MF_MT_AVG_BITRATE`) and no quality modes;
//!   CQP/CRF would need `ICodecAPI`, which this crate does not use.
//! - `AudioMode::System` / `Mic` / `Both` — the library has no audio capture
//!   source (no loopback, no mic), so the audio track is always disabled and
//!   only `AudioMode::None` produces a recording.
//! - hardware scaling — a resolution change is a CPU BGRA resize
//!   ([`engine::resize_bgra`]) per frame, not an encoder-side crop/pad or GPU
//!   stretch.
//! - frame-rate guarantee — `MinimumUpdateIntervalSettings::Custom` is a hint;
//!   the engine paces frames itself ([`engine::should_send`]). On OS builds
//!   without the `MinUpdateInterval` WinRT property the hint is not asked for
//!   at all (`Recorder::new` probes it) and pacing stays timestamp-based.

#![forbid(unsafe_code)]

pub mod config;
pub mod engine;
pub mod error;
pub mod ring;

pub use engine::Recorder;
