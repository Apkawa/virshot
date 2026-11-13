//! egui test example for the `video-capture` crate (video-capture plan Stage 3).
//!
//! Exposes every spec parameter with honest states: the controls that
//! `CaptureConfig` cannot honor (audio sources) stay visible but disabled or
//! annotated, and the rest are the ones the engine actually applies — codec, CBR
//! bitrate, resolution, scale filter, fps, replay — so a Windows rig sees what
//! is actually applied rather than what the UI merely remembers.
//!
//! Run on Windows: `./cargo.sh run --target x86_64-pc-windows-gnu -p video-capture --example capture_demo`.
//! On Linux the UI compiles and opens (`./cargo.sh run -p video-capture --example
//! capture_demo`, e.g. under `xvfb-run -a`); `Recorder::recording` reports
//! [`video_capture::error::CaptureError::UnsupportedPlatform`] as a status line.
//!
//! The facade is non-blocking: «Запись» queues `Start` — on the session that is
//! already live (`Recorder::start`) or on a freshly built one
//! (`Recorder::recording`) — «Стоп» only queues the end of the recording, «Буфер»
//! toggles the pre-roll of a live session or boots a capture-only one
//! (`Recorder::new`), and both acks (`poll_recording_ack` / `poll_stop_ack`) are
//! polled on every update pass. Nothing ever waits for a captured frame, encoder
//! activation, or file muxing, so the window keeps responding while the capture
//! thread works.
//!
//! The engine releases the capture session when the replay ring has no capacity
//! and nothing will fill it again: a recording that stops with the ring
//! disabled, and «Буфер» switched off while no recording is active (that
//! release lands on the next frame). The capture thread ends, the Windows
//! capture indicator disappears, and the facade refuses further
//! `start`/`stop`/`enable_replay`/`disable_replay` with
//! [`video_capture::error::CaptureError::NotConfigured`]. The demo keeps that
//! released recorder only to say what happened: `poll_acks` carries the release
//! after a stop, `poll_session_release` the one triggered by the buffer toggle,
//! and the next «Запись» or «Буфер» builds a fresh session.
//!
//! Logs go to stdout (INFO+) and stderr (WARN+): eframe itself logs to stdout,
//! and the Windows rig reads both streams.
//!
//! The debug checkbox feeds `Recorder::with_debug_dump`, which dumps the first
//! three frames of the raw-buffer path as PNGs so the exact bytes handed to
//! `send_frame_buffer` can be inspected on a Windows rig. That path has no
//! pre-queued boot recording, so its recording is queued through
//! [`Recorder::start`] — queued, not live, exactly like the normal path.

#![forbid(unsafe_code)]

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use egui::{CentralPanel, DragValue, Slider, Ui, ViewportBuilder};
use video_capture::Recorder;
use video_capture::config::{AudioMode, CaptureConfig, Codec, Resolution, ScaleFilter};
use video_capture::engine::config_dump;
use video_capture::error::CaptureError;
use video_capture::ring::estimated_memory_bytes;

/// Bitrate bounds the encoder is asked within (kbps), matching the spec slider.
const MIN_BITRATE_KBPS: u32 = 500;
const MAX_BITRATE_KBPS: u32 = 50_000;

/// FPS bounds the demo allows. Zero is not a frame rate: the `Recorder`
/// constructors refuse it (`CaptureConfig::invalid_reason`), so the control
/// never lets the UI ask for one.
const MIN_FPS: u8 = 1;
const MAX_FPS: u8 = 60;

/// Height bounds the demo allows, in pixels. Zero is not a height: the
/// `Recorder` constructors refuse it (`CaptureConfig::invalid_reason`), so the
/// field starts at 1. The upper bound is 4× Full HD — above the native desktop
/// the engine keeps the native size, so a larger value costs nothing rather
/// than upscaling.
const MIN_HEIGHT_PX: u16 = 1;
const MAX_HEIGHT_PX: u16 = 4320;

/// Height the numeric field starts with, mirroring `Resolution::P1080`
/// (`CaptureConfig::default()`).
const DEFAULT_HEIGHT_PX: u16 = 1080;

/// Replay pre-roll bounds, in seconds — the same clamp `CaptureConfig` applies.
const MAX_REPLAY_SECONDS: u32 = 600;

/// Native-size hint for the replay memory estimate: the real desktop size is
/// only known inside the engine (`Monitor::primary()`, Windows), so the demo
/// estimates against a 1080p desktop and the label says so.
const NATIVE_HINT: (u32, u32) = (1920, 1080);

/// Debug dump directory for the raw-buffer path, passed to
/// `Recorder::with_debug_dump` (the engine creates it; a failure there is a
/// warning, never a capture failure).
const DEBUG_DUMP_DIR: &str = ".tmp/dumps";

/// Repaint cadence for the demo loop (task 2026-11-13_gpu-repaint-throttle,
/// Stage 4). Nothing here animates — no spinner, no progress bar — so a
/// per-frame repaint only bought GPU work for a status line that changes at
/// ack speed. 200 ms mirrors the shipped throttle
/// (`crates/overlay/src/host.rs::REPAINT_INTERVAL`,
/// `src/gui/app.rs::CHILD_VIEWPORT_REPAINT_INTERVAL`) and stays above eframe's
/// invisible-window floor of 100 ms (`run.rs:27`, applied `run.rs:376-382`),
/// so the interval is not silently raised for a hidden window.
///
/// egui keeps the EARLIEST scheduled repaint (strict `delay < repaint_delay`)
/// and a non-zero delay buys exactly ONE repaint, so `App::ui` re-issues this
/// request every pass; an input event still repaints immediately because
/// `Duration::ZERO` wins the `<`.
const REPAINT_INTERVAL: Duration = Duration::from_millis(200);

/// Russian hint for the FPS control: the configured rate is a hint to Media
/// Foundation (`MinimumUpdateIntervalSettings::Custom`), and the engine paces
/// frames by timestamp itself, so fewer frames than asked can arrive.
const FPS_HINT: &str = "Частота кадров — подсказка для Media Foundation: реальная доставка может быть ниже запрошенной; движок ограничивает по таймстампам.";
/// Russian hint for the resolution controls: what the engine actually does with
/// a height. The width comes from the desktop's aspect ratio
/// (`Resolution::target_size`), a height above the native one is not upscaled,
/// `Height(0)` is refused by the constructors (`CaptureConfig::invalid_reason`)
/// so the field starts at 1, and the target is fixed when the recorder is built
/// (README: a size change only takes effect with a new `Recorder`).
const RESOLUTION_HINT: &str = "Ширина выводится из пропорций экрана: высота 160 на 1920×1080 даёт 284×160. Высота выше родного разрешения не апскейлится — остаётся родной размер. Высота 0 недопустима: поле ограничено от 1. Ресайз — CPU на каждый кадр, не кроп энкодера. Целевой размер фиксируется при создании recorder: смена высоты действует только со следующим «Запись» или «Буфер», когда демо строит новый recorder.";

/// Russian hint while «Полный размер экрана (Full)» is checked: the frame keeps
/// the monitor's native size, so there is no height to set and no CPU resize.
const FULL_RESOLUTION_HINT: &str =
    "Full: кадр нативного размера монитора — высота не задаётся, ресайз не делается.";

const AUDIO_HINT: &str = "Not implemented: windows-capture grabs no audio — system/mic/both need a separate source dependency.";
const DEBUG_DUMP_HINT: &str = "Dump the first 3 buffer-path frames (target size != native size) as raw/flipped/rgba PNGs — orientation, stride, and channel evidence for send_frame_buffer.";

/// Russian hint for the «Буфер» toggle: the engine does not push frames into the
/// replay ring while a recording is active, so a toggle during recording only
/// takes effect once the ring starts filling again.
const REPLAY_RING_HINT: &str = "Пока запись активна, кольцо не наполняется: оно возобновляется после «Стоп», поэтому переключение «Буфер» во время записи задаёт пре-ролл для следующей записи.";

/// Russian hint shown while no capture session exists: «Буфер» boots one now,
/// and the slider sets the ring it starts with.
const BUFFER_NO_SESSION_HINT: &str =
    "Нет сессии захвата: «Буфер» запускает её, а ползунок задаёт размер кольца.";

/// Russian hint for a zero slider with no session: nothing can be pre-rolled, so
/// «Буфер» deliberately does not boot a capture session in that state.
const REPLAY_ZERO_HINT: &str =
    "Ползунок 0: пре-ролла нет — кольцу нечего наполнять, поэтому «Буфер» не запускает сессию.";

/// Russian hint for a released session: the engine stopped capture because the
/// ring had no capacity — either «Стоп» with the buffer off, or «Буфер»
/// switched off while no recording was running — so the next command builds a
/// fresh session instead of reusing the recorder that is now a dead handle.
const RELEASED_SESSION_HINT: &str = "Захват освобождён: после «Стоп» с выключенным буфером или после выключения «Буфера» без записи индикатор Windows погас; «Буфер» или «Запись» поднимут новую сессию.";

/// Linux-only honesty note: the facade starts no capture thread there, so a
/// session the demo booted never actually captures — the status line describes
/// requested state, not a running capture.
#[cfg(not(windows))]
const LINUX_NO_CAPTURE_HINT: &str = "Linux: захват не поддерживается — сессия не запускается, статус описывает только запрошенное состояние.";

/// What the demo currently does, shown as the status line.
///
/// The pending states are what the non-blocking facade makes necessary: a
/// queued command is not a live recording, so the UI says «запуск»/«завершение»
/// until the corresponding ack arrives.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Status {
    /// Nothing started — no recorder, no capture session.
    Idle,
    /// Capture session running, no recording. `last_file` is the mp4 of the
    /// recording that just finished, so the line reads «файл записан: …» while
    /// the ring keeps filling. `released` says the engine ended the capture
    /// session because the ring had no capacity: the recording stopped with the
    /// ring disabled, or the buffer was switched off while no recording was
    /// running. The recorder is kept alive only as a record of what happened,
    /// and the next command builds a fresh session.
    Capturing {
        last_file: Option<String>,
        released: bool,
    },
    /// `Start` is queued (`Recorder::recording` or `Recorder::start`); recording
    /// activates on the first captured frame, so this is not yet a live file.
    StartPending { path: String },
    /// The `RecordingStarted` ack arrived — the recording is live.
    Recording { path: String },
    /// `Stop` is queued; the capture thread is finishing the encoder and muxing
    /// the file, so the path is not written yet.
    StopPending { path: String },
    /// The last failure, as `CaptureError`'s `Display` text.
    Failed { message: String },
}

/// Default output path: `%USERPROFILE%\Videos\test.mp4` on Windows (plain env
/// expansion — the crate has no home-dir dependency), `test.mp4` beside the
/// current directory when `USERPROFILE` is missing or on Linux.
fn default_output_path() -> String {
    #[cfg(windows)]
    {
        let profile = std::env::var("USERPROFILE")
            .ok()
            .filter(|value| !value.is_empty());
        if let Some(profile) = profile {
            return PathBuf::from(profile)
                .join("Videos")
                .join("test.mp4")
                .display()
                .to_string();
        }
    }

    // Linux (and Windows without USERPROFILE): a file beside the cwd.
    "test.mp4".to_string()
}

/// Display label for an audio source.
fn audio_label(mode: AudioMode) -> &'static str {
    match mode {
        AudioMode::None => "none",
        AudioMode::System => "system",
        AudioMode::Mic => "mic",
        AudioMode::Both => "system + mic",
    }
}

/// Display label for a codec choice.
fn codec_label(codec: Codec) -> &'static str {
    match codec {
        Codec::H264 => "h.264",
        Codec::Hevc => "HEVC (h.265)",
    }
}

/// Display label for a resolution choice. A fixed height reads as `"{h}p"`
/// via `Resolution::label`; `Full` keeps the demo's native-size wording. The
/// replay estimate uses it to name the choice it priced.
fn resolution_label(resolution: Resolution) -> String {
    match resolution {
        Resolution::Full => "Full (native)".to_string(),
        other => other.label(),
    }
}

/// Per-filter cost/quality note for the «Scale filter» dropdown. The numbers are
/// measured on a 1680x1050 -> 1152x720 frame (release build, Linux host); the
/// own kernels (`Nearest`, `Bilinear`) are the crate's, the rest go through
/// `image::imageops::resize`.
fn scale_filter_hint(filter: ScaleFilter) -> &'static str {
    match filter {
        ScaleFilter::Nearest => {
            "Свой kernel, ~4 мс/кадр: самый дешёвый, но тонкие штрихи шрифта теряются при даунскейле."
        }
        ScaleFilter::Bilinear => {
            "Свой kernel (bilinear), ~12 мс/кадр: дефолт — штрихи сохраняются, лёгкий блюр."
        }
        ScaleFilter::CatmullRom => {
            "Через image, ~116 мс/кадр: резче bilinear, но это ~8 fps на CPU."
        }
        ScaleFilter::Gaussian => "Через image, ~157 мс/кадр: гладче, но это ~6 fps на CPU.",
        ScaleFilter::Lanczos3 => {
            "Через image, ~156 мс/кадр: максимально резкий даунскейл, но это ~6 fps на CPU."
        }
    }
}

/// Replay memory estimate in MiB, against the native-size hint (`NATIVE_HINT`):
/// the engine measures the real desktop inside the capture thread, so every
/// estimate here is an estimate, not a measurement.
fn replay_mib(replay_seconds: u32, fps: u8, resolution: Resolution) -> f64 {
    let (width, height) = resolution.target_size(NATIVE_HINT);
    estimated_memory_bytes(replay_seconds, u32::from(fps), width, height) as f64 / (1024.0 * 1024.0)
}

/// Replay memory estimate text for the current parameters. Honest: the ring is
/// priced as `width * height * 4 bytes * fps * seconds` for the target size
/// `Resolution::target_size` derives from the native hint, and the demo never
/// sees the real desktop — the engine reads it inside the capture thread on
/// Windows and logs it there — so the native size in this label is a hint, not
/// a measurement.
fn replay_estimate(replay_seconds: u32, fps: u8, resolution: Resolution) -> String {
    let (width, height) = resolution.target_size(NATIVE_HINT);
    // Locals, not the const: format strings capture variables, not consts.
    let (hint_width, hint_height) = NATIVE_HINT;
    format!(
        "оценка памяти: ~{:.1} MiB (кадр {width}x{height} — {}; реальный размер экрана движок читает на Windows, здесь подсказка {hint_width}x{hint_height})",
        replay_mib(replay_seconds, fps, resolution),
        resolution_label(resolution),
    )
}

/// The demo window: parameters on the left, start/stop driving a `Recorder`.
struct CaptureDemo {
    output_path: String,
    codec: Codec,
    bitrate_kbps: u32,
    /// «Полный размер экрана (Full)»: checked means `Resolution::Full`, and the
    /// height field is hidden because there is no height to choose.
    full: bool,
    /// Target frame height in pixels, used when `full` is unchecked; the width
    /// is derived from the desktop's aspect ratio by the engine.
    height: u16,
    scale_filter: ScaleFilter,
    fps: u8,
    replay_seconds: u32,
    audio: AudioMode,
    /// Debug aid: dumps the raw-buffer frames via `Recorder::with_debug_dump`.
    debug_dump: bool,
    recorder: Option<Recorder>,
    /// Pre-roll seconds the live recorder was last asked for: the startup config
    /// at boot, or the seconds of the latest `enable_replay`/`disable_replay`
    /// request. `None` while no recorder exists — the status line must not
    /// describe a buffer the engine was never given.
    replay_applied: Option<u32>,
    status: Status,
}

impl Default for CaptureDemo {
    fn default() -> Self {
        // Defaults mirror `CaptureConfig::default()` so the demo shows what the
        // library defaults to, not demo-specific values: `Full` maps to the
        // checkbox, a fixed height maps to the numeric field (a `Full` default
        // keeps the field at `DEFAULT_HEIGHT_PX` for the unchecked case).
        let config = CaptureConfig::default();
        let (full, height) = match config.resolution {
            Resolution::Full => (true, DEFAULT_HEIGHT_PX),
            Resolution::Height(height) => (false, height),
        };
        Self {
            output_path: default_output_path(),
            codec: config.codec,
            bitrate_kbps: config.bitrate_kbps,
            full,
            height,
            scale_filter: config.scale_filter,
            fps: config.fps,
            replay_seconds: config.replay_seconds,
            audio: config.audio,
            debug_dump: false,
            recorder: None,
            replay_applied: None,
            status: Status::Idle,
        }
    }
}

impl CaptureDemo {
    /// The resolution the two widgets describe: `Full` while the checkbox is
    /// checked, otherwise the height from the numeric field. The engine turns it
    /// into a target size when the recorder is built, so the widgets themselves
    /// store no preset.
    fn resolution(&self) -> Resolution {
        if self.full {
            Resolution::Full
        } else {
            Resolution::Height(self.height)
        }
    }

    /// The configuration the UI currently describes. Every field here is one the
    /// engine honors: codec, bitrate (CBR), resolution, scale filter, fps, replay,
    /// audio. An unsupported `audio` is reported by the engine's dump.
    fn config(&self) -> CaptureConfig {
        CaptureConfig {
            codec: self.codec,
            bitrate_kbps: self.bitrate_kbps,
            resolution: self.resolution(),
            scale_filter: self.scale_filter,
            fps: self.fps,
            replay_seconds: self.replay_seconds,
            audio: self.audio,
        }
    }

    /// «Запись»: start a recording, reusing the capture session that is already
    /// live. `Recorder::start` only queues the command and returns immediately,
    /// so the recording activates on the next captured frame while the running
    /// capture thread, its armed ring, and the Windows capture indicator stay
    /// untouched.
    ///
    /// A fresh `Recorder` is built only when there is no session at all, when the
    /// engine released the previous one (a stop with the ring disabled, or a
    /// buffer switched off while idle), or when the debug checkbox asks for
    /// dumps: the dump directory is a constructor argument, so a session that is
    /// already running cannot be asked to dump. The previous recorder (if any)
    /// shuts down and joins its capture thread on drop — that also finishes any
    /// live recording.
    ///
    /// Nothing blocks: `Recorder::recording` enqueues `Start` *before* the capture
    /// thread starts, so the recording activates on the first captured frame. The
    /// demo does not claim a live recording until `poll_recording_ack` reports the
    /// ack, so the status line stays «запуск записи…» while the encoder is built.
    fn start_recording(&mut self) {
        let cfg = self.config();
        // Debugging aid: every parameter as `CaptureConfig` describes it, plus
        // markers for the ones the library cannot honor, logged before the
        // recorder is built (on Linux too, ahead of `UnsupportedPlatform`).
        tracing::info!("starting recording with parameters: {}", config_dump(&cfg));

        let path = PathBuf::from(&self.output_path);
        tracing::info!(path = %path.display(), "recording output path");

        if self.debug_dump {
            tracing::info!(dir = DEBUG_DUMP_DIR, "debug frame dumps requested");
        }

        // The engine rejects a missing parent directory, so the demo creates it
        // up front (startup context) and reports the failure instead of panicking.
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            let message = format!(
                "cannot create output directory {}: {error}",
                parent.display()
            );
            tracing::error!(%error, path = %path.display(), "output directory unavailable");
            self.status = Status::Failed { message };
            return;
        }

        // A live session that the engine has not released keeps its capture
        // thread: the new recording is queued on it, nothing is rebuilt, and the
        // ring stays armed. `CaptureError::NotConfigured` says the session is
        // already gone (the facade refuses commands on a released one), so that
        // outcome is a rebuild below, not a failure the user has to clear.
        if self.live_session()
            && !self.debug_dump
            && let Some(recorder) = self.recorder.as_mut()
        {
            // The recorder takes ownership of the path; the demo keeps the
            // same path for the status line and for matching the acks, hence
            // the clone.
            match recorder.start(path.clone()) {
                Ok(()) => {
                    self.status = Status::StartPending {
                        path: path.display().to_string(),
                    };
                    tracing::info!(
                        path = %path.display(),
                        replay_seconds = cfg.replay_seconds(),
                        "recording queued on the live session; waiting for the start ack"
                    );
                    return;
                }
                Err(CaptureError::NotConfigured) => {
                    tracing::info!(
                        path = %path.display(),
                        "capture session was released; building a fresh one"
                    );
                }
                Err(error) => {
                    tracing::error!(%error, path = %path.display(), "recording start could not be queued");
                    self.status = Status::Failed {
                        message: error.to_string(),
                    };
                    return;
                }
            }
        }

        // Fresh session: the recorder from before — released or still running —
        // is dropped here, which stops and joins its capture thread.
        self.recorder = None;
        self.replay_applied = None;

        // The dump choice is decided here because the engine resolves the dump
        // directory at construction: the constructor argument wins over the
        // `VC_DEBUG_DUMP_DIR` environment variable. `Recorder::recording` has no
        // dump argument, so the debug path boots a ring-only session and queues
        // `Start` through `Recorder::start` below — both paths end up with the
        // same pending ack, and neither one waits for it.
        let built = if self.debug_dump {
            Recorder::with_debug_dump(cfg, Some(PathBuf::from(DEBUG_DUMP_DIR)))
        } else {
            // The recorder takes ownership of the path; the demo keeps the same
            // path for the status line and for matching the acks, hence the clone.
            Recorder::recording(cfg, path.clone())
        };

        let mut recorder = match built {
            Ok(recorder) => recorder,
            Err(error) => {
                tracing::error!(%error, "capture could not start");
                self.status = Status::Failed {
                    message: error.to_string(),
                };
                return;
            }
        };

        // Debug path only: `Recorder::start` returns as soon as the command is
        // queued, so it never waits for the encoder (unlike the old blocking
        // `new` + `start` pair). A queue failure drops the recorder here:
        // `Recorder::Drop` stops the capture thread, so no thread is left running.
        if self.debug_dump
            && let Err(error) = recorder.start(path.clone())
        {
            tracing::error!(%error, path = %path.display(), "recording start could not be queued");
            self.status = Status::Failed {
                message: error.to_string(),
            };
            return;
        }

        self.replay_applied = Some(cfg.replay_seconds());
        self.recorder = Some(recorder);
        self.status = Status::StartPending {
            path: path.display().to_string(),
        };
        tracing::info!(
            path = %path.display(),
            replay_seconds = cfg.replay_seconds(),
            "recording queued; waiting for the start ack"
        );
    }

    /// «Буфер» with no live session: boot a capture-only session whose ring is
    /// armed by the startup config, so no `enable_replay` call is needed — the
    /// config already tells the engine how many seconds to pre-roll. The ring
    /// fills from the first captured frame; no recording is queued.
    ///
    /// A released session — from a previous «Стоп» or from switching «Буфер» off
    /// while idle — is dropped here: its thread has already ended, so
    /// `Recorder::Drop` just joins it.
    fn start_capture_session(&mut self) {
        let cfg = self.config();
        tracing::info!(
            "starting capture-only session with parameters: {}",
            config_dump(&cfg)
        );

        self.recorder = None;
        self.replay_applied = None;

        match Recorder::new(cfg) {
            Ok(recorder) => {
                self.recorder = Some(recorder);
                self.replay_applied = Some(cfg.replay_seconds());
                self.status = Status::Capturing {
                    last_file: None,
                    released: false,
                };
                tracing::info!(
                    replay_seconds = cfg.replay_seconds(),
                    "capture-only session started; the ring fills from the first frame"
                );
            }
            Err(error) => {
                tracing::error!(%error, "capture session could not start");
                self.status = Status::Failed {
                    message: error.to_string(),
                };
            }
        }
    }

    /// Whether a capture session is still running: a recorder exists and the
    /// engine has not released it. A released recorder answers every command
    /// with `CaptureError::NotConfigured`, so «Запись» and «Буфер» build a fresh
    /// session instead of reusing it.
    fn live_session(&self) -> bool {
        match (&self.recorder, &self.status) {
            (Some(_), Status::Capturing { released: true, .. }) => false,
            (Some(_), _) => true,
            (None, _) => false,
        }
    }

    /// «Стоп»: queue the end of the recording. Capture keeps running so the replay
    /// ring stays warm, and the encoder is finished and the file muxed on the
    /// capture thread — so the status stays «завершение, запись файла…» until
    /// `poll_stop_ack` reports the ack.
    fn stop_recording(&mut self) {
        // The button is enabled only while a recording is live, so the path the
        // ack belongs to is taken from the current status.
        let Some(path) = self.recording_path() else {
            self.status = Status::Idle;
            return;
        };

        let Some(recorder) = self.recorder.as_mut() else {
            // No session to stop: nothing was queued.
            self.status = Status::Idle;
            return;
        };

        match recorder.stop() {
            Ok(()) => {
                tracing::info!(path = %path, "recording stop queued; waiting for the stop ack");
                self.status = Status::StopPending { path };
            }
            Err(error) => {
                tracing::error!(%error, path = %path, "recording stop could not be queued");
                // The recorder stays alive: the capture thread is still running,
                // so «Буфер» keeps working and «Запись» can start a fresh session.
                self.status = Status::Failed {
                    message: error.to_string(),
                };
            }
        }
    }

    /// Path of the live recording, if the status says one is running.
    fn recording_path(&self) -> Option<String> {
        match &self.status {
            Status::Recording { path } => Some(path.clone()),
            _ => None,
        }
    }

    /// Path of the recording whose `Start` ack is still pending.
    fn start_pending_path(&self) -> Option<String> {
        match &self.status {
            Status::StartPending { path } => Some(path.clone()),
            _ => None,
        }
    }

    /// Path of the recording whose `Stop` ack is still pending.
    fn stop_pending_path(&self) -> Option<String> {
        match &self.status {
            Status::StopPending { path } => Some(path.clone()),
            _ => None,
        }
    }

    /// Poll both queued-command acks once, without blocking. Called on every
    /// update pass: `None` means still pending, so the state stays as it is and
    /// the next pass asks again. Neither poll waits — they drain the event
    /// channel with `try_recv`, and a pending deadline is reported as `Some(Err)`.
    fn poll_acks(&mut self) {
        // Exactly one of the two states can be pending at a time.
        if let Some(path) = self.start_pending_path() {
            let outcome = self
                .recorder
                .as_mut()
                .and_then(Recorder::poll_recording_ack);
            match outcome {
                Some(outcome) => self.on_recording_ack(&path, outcome),
                None => {
                    // A pending ack with no recorder can never arrive; the session
                    // is gone, so the demo falls back to idle rather than waiting.
                    if self.recorder.is_none() {
                        tracing::warn!("start ack pending but the recorder is gone");
                        self.status = Status::Idle;
                    }
                }
            }
            return;
        }

        if let Some(path) = self.stop_pending_path() {
            let outcome = self.recorder.as_mut().and_then(Recorder::poll_stop_ack);
            match outcome {
                Some(outcome) => self.on_stop_ack(&path, outcome),
                None => {
                    if self.recorder.is_none() {
                        tracing::warn!("stop ack pending but the recorder is gone");
                        self.status = Status::Idle;
                    }
                }
            }
        }
    }

    /// Non-blocking release poll for a live session with no queued command:
    /// switching the buffer off with no recording running releases the session
    /// on the next frame, and that release is not an ack for any command, so no
    /// ack poll carries it. Called on every update pass next to
    /// [`CaptureDemo::poll_acks`]: while a `Start`/`Stop` ack is pending the
    /// facade reads nothing, so the ack is never swallowed and the release is
    /// picked up on a later pass. Only a live `Capturing` status can flip to
    /// `released`; pending, failed, and idle states are left untouched.
    fn poll_session_release(&mut self) {
        // Status is cloned so reading it does not span the mutable borrow of the
        // recorder below; the enum is small, so the clone costs nothing.
        let Status::Capturing {
            last_file,
            released,
        } = self.status.clone()
        else {
            return;
        };
        if released {
            // Already reported; the released recorder answers with the flag, so
            // re-polling would log nothing new.
            return;
        }

        let Some(recorder) = self.recorder.as_mut() else {
            return;
        };

        if recorder.poll_session_release() {
            tracing::info!(
                "capture session released: the capture thread stopped and the Windows capture indicator is gone"
            );
            self.status = Status::Capturing {
                last_file,
                released: true,
            };
        }
    }

    /// Outcome of the queued `Start`: the recording is live only on the ack. A
    /// failure event or an expired deadline drops the recorder — `Recorder::Drop`
    /// stops the capture thread — and shows the error, so a failed start never
    /// leaves a thread running or a file claimed.
    fn on_recording_ack(&mut self, path: &str, outcome: Result<(), CaptureError>) {
        match outcome {
            Ok(()) => {
                tracing::info!(path, "recording acked: encoder live");
                self.status = Status::Recording {
                    path: path.to_string(),
                };
            }
            Err(error) => {
                tracing::error!(%error, path, "recording start failed");
                self.recorder = None;
                self.replay_applied = None;
                self.status = Status::Failed {
                    message: error.to_string(),
                };
            }
        }
    }

    /// Outcome of the queued `Stop`: on the ack the file is muxed and closed, so
    /// the line reads «файл записан: …». With the ring enabled the engine keeps
    /// capturing and the ring fills again from the next frame; with the ring
    /// disabled the engine releases the session (the capture thread ends and the
    /// Windows capture indicator disappears), and the line says so. The release
    /// event can arrive after the stop ack, so the release poll keeps confirming
    /// it on later passes. The recorder stays alive in both cases: `shutdown`/
    /// `Drop` on a session whose thread already ended only joins it, so keeping
    /// it costs nothing and lets the status line name what happened. A failure
    /// event or an expired deadline shows the error and re-enables the buttons.
    fn on_stop_ack(&mut self, path: &str, outcome: Result<(), CaptureError>) {
        match outcome {
            Ok(()) => {
                // Prefer the real signal: `poll_session_release` reports the
                // `CaptureReleased` the facade already recorded while polling
                // the stop ack, and drains it when the event arrives *after*
                // that ack — the stop ack is consumed here, so nothing pending
                // is swallowed. The zero-capacity check stays as the fallback:
                // a stop with the ring disabled releases deterministically,
                // even if the release event never reaches the channel.
                let released = self.recorder.as_mut().is_some_and(|recorder| {
                    recorder.poll_session_release() || !recorder.replay_enabled()
                });
                tracing::info!(path, released, "recording muxed and closed");
                self.status = Status::Capturing {
                    last_file: Some(path.to_string()),
                    released,
                };
            }
            Err(error) => {
                tracing::error!(%error, path, "recording stop failed");
                self.status = Status::Failed {
                    message: error.to_string(),
                };
            }
        }
    }

    /// «Буфер»: with a live session, toggle the runtime pre-roll of that session;
    /// with no session — none ever started, or one the engine released after
    /// «Стоп» or after a buffer switch-off — boot a capture-only session whose
    /// ring the slider sizes.
    ///
    /// A zero slider boots nothing: an empty ring has nothing to pre-roll, so the
    /// button is disabled and the hint explains why rather than faking a buffer.
    ///
    /// Turning the buffer off with no recording running releases the session on
    /// the **next captured frame**; with a recording active the engine keeps the
    /// session open and releases it only when the file is muxed. Nothing is
    /// claimed here — the release poll renders the outcome on the next pass.
    fn toggle_replay(&mut self) {
        if !self.live_session() {
            if self.replay_seconds == 0 {
                tracing::info!(
                    "buffer requested with the slider at 0: nothing to pre-roll, no session booted"
                );
                return;
            }
            self.start_capture_session();
            return;
        }

        // `live_session` already guarantees a recorder; the re-check keeps the
        // borrow of the recorder out of the paths below.
        let Some(recorder) = self.recorder.as_mut() else {
            return;
        };

        let enabling = !recorder.replay_enabled();
        let seconds = if enabling { self.replay_seconds } else { 0 };
        tracing::info!(enabling, seconds, "replay buffer toggle requested");

        let outcome = if enabling {
            recorder.enable_replay(seconds)
        } else {
            recorder.disable_replay()
        };

        match outcome {
            // A delivered request changes the facade's reported state at once; the
            // ring itself changes on the next captured frame.
            Ok(()) => {
                self.replay_applied = Some(seconds);
                // With no recording active the engine releases the capture
                // session on that next frame (spec v4 item 2); nothing is
                // claimed here — the release poll renders it when it lands.
                // With a recording active the session stays open until the
                // file is muxed.
                if !enabling && matches!(self.status, Status::Capturing { .. }) {
                    tracing::info!(
                        "buffer disabled with no recording; the engine releases the session on the next frame"
                    );
                }
            }
            Err(error) => {
                tracing::error!(%error, seconds, "replay toggle could not be queued");
                self.status = Status::Failed {
                    message: error.to_string(),
                };
            }
        }
    }

    /// Replay part of the status line. Honest: while the buffer is off — or the
    /// window the recorder was asked for is `0` — nothing is claimed about the ring.
    fn replay_line(&self) -> String {
        let Some(recorder) = self.recorder.as_ref() else {
            return "буфер выключен".to_string();
        };

        let Some(seconds) = self.replay_applied.filter(|seconds| *seconds > 0) else {
            return "буфер выключен".to_string();
        };

        if !recorder.replay_enabled() {
            return "буфер выключен".to_string();
        }

        // The estimate uses the recorder's own startup config, not the UI values
        // a user may have changed since the session was booted.
        let config = recorder.config();
        format!(
            "буфер {seconds} с (~{:.1} MiB)",
            replay_mib(seconds, config.fps, config.resolution)
        )
    }

    /// Status line for the current state. The replay part is shown only where the
    /// ring is actually being fed: during a recording the engine does not push
    /// frames into the ring, and a released session has no ring and no thread.
    fn status_line(&self) -> String {
        match &self.status {
            Status::Idle => "ожидание".to_string(),
            Status::Capturing {
                last_file,
                released,
            } => {
                let written = match last_file {
                    Some(file) => format!("файл записан: {file}, "),
                    None => String::new(),
                };

                if *released {
                    // No capture thread anymore: the ring had no capacity, so
                    // the engine released the session — either «Стоп» with the
                    // ring disabled, or «Буфер» switched off while no recording
                    // was running.
                    format!("{written}захват освобождён (буфер выключен)")
                } else {
                    let replay = self.replay_line();
                    match last_file {
                        Some(_) => {
                            format!("{written}захват активен, {replay}; кольцо наполняется заново")
                        }
                        None => format!("захват активен, {replay}"),
                    }
                }
            }
            Status::StartPending { path } => format!("запуск записи… {path}"),
            Status::Recording { path } => format!("запись активна: {path}"),
            Status::StopPending { path } => format!("завершение, запись файла… {path}"),
            Status::Failed { message } => format!("ошибка: {message}"),
        }
    }

    /// Parameter controls: one row per spec parameter, disabled where the
    /// library cannot honor the choice.
    fn parameters(&mut self, ui: &mut Ui) {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("Output file");
                ui.text_edit_singleline(&mut self.output_path);
            });

            egui::ComboBox::from_label("Codec")
                .selected_text(codec_label(self.codec))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.codec, Codec::H264, codec_label(Codec::H264));
                    ui.selectable_value(&mut self.codec, Codec::Hevc, codec_label(Codec::Hevc));
                });

            ui.horizontal(|ui| {
                ui.label("Bitrate");
                ui.add(
                    DragValue::new(&mut self.bitrate_kbps)
                        .range(MIN_BITRATE_KBPS..=MAX_BITRATE_KBPS)
                        .suffix(" kbps"),
                );
                ui.small("target CBR bitrate (MF_MT_AVG_BITRATE)");
            });

            ui.checkbox(&mut self.full, "Полный размер экрана (Full)");
            if self.full {
                ui.small(FULL_RESOLUTION_HINT);
            } else {
                ui.horizontal(|ui| {
                    ui.label("Высота кадра (px)");
                    ui.add(DragValue::new(&mut self.height).range(MIN_HEIGHT_PX..=MAX_HEIGHT_PX));
                });
            }
            ui.small(RESOLUTION_HINT);

            egui::ComboBox::from_label("Scale filter")
                .selected_text(self.scale_filter.label())
                .show_ui(ui, |ui| {
                    for filter in [
                        ScaleFilter::Nearest,
                        ScaleFilter::Bilinear,
                        ScaleFilter::CatmullRom,
                        ScaleFilter::Gaussian,
                        ScaleFilter::Lanczos3,
                    ] {
                        ui.selectable_value(&mut self.scale_filter, filter, filter.label());
                    }
                });
            ui.small(scale_filter_hint(self.scale_filter));

            ui.horizontal(|ui| {
                ui.label("FPS");
                ui.add(
                    DragValue::new(&mut self.fps)
                        .range(MIN_FPS..=MAX_FPS)
                        .suffix(" fps"),
                );
                ui.small(FPS_HINT);
            });

            ui.add(
                Slider::new(&mut self.replay_seconds, 0..=MAX_REPLAY_SECONDS).suffix(" s replay"),
            );
            ui.small(replay_estimate(
                self.replay_seconds,
                self.fps,
                self.resolution(),
            ));

            egui::ComboBox::from_label("Audio")
                .selected_text(audio_label(self.audio))
                .show_ui(ui, |ui| {
                    // Only `None` records: the library has no audio source, so the
                    // rest are shown disabled instead of pretending to work.
                    for mode in [
                        AudioMode::None,
                        AudioMode::System,
                        AudioMode::Mic,
                        AudioMode::Both,
                    ] {
                        ui.add_enabled_ui(mode.is_supported(), |ui| {
                            ui.selectable_value(&mut self.audio, mode, audio_label(mode));
                        });
                    }
                });
            if !self.audio.is_supported() {
                ui.small(AUDIO_HINT);
            }

            // Debug aid: `Recorder::with_debug_dump` is chosen by this box, so
            // it has an effect on the next «Запись», not on a live recording.
            ui.checkbox(&mut self.debug_dump, "Debug: dump кадров в .tmp/dumps");
            if self.debug_dump {
                ui.small(DEBUG_DUMP_HINT);
            }
        });
    }

    /// Buttons + status line. Each button says what is actually live: a queued
    /// command is not a recording, «Запись» reuses the session that is already
    /// running (and rebuilds only when there is none or the engine released it),
    /// and «Буфер» toggles a live session or boots a capture-only one.
    fn actions(&mut self, ui: &mut Ui) {
        // Whether the engine still runs a capture session: decides what «Буфер»
        // does (toggle vs boot) and which hint is honest.
        let live_session = self.live_session();

        ui.horizontal(|ui| {
            // A queued command or a live recording is work the capture thread is
            // already doing, so «Запись» must not stack another one on top of it.
            let session_busy = matches!(
                self.status,
                Status::StartPending { .. } | Status::Recording { .. } | Status::StopPending { .. }
            );
            ui.add_enabled_ui(!session_busy, |ui| {
                if ui.button("Запись").clicked() {
                    self.start_recording();
                }
            });

            // «Стоп» is meaningful only while a recording is live; capture itself
            // keeps running after the ack, so the recorder stays alive.
            let recording = matches!(self.status, Status::Recording { .. });
            ui.add_enabled_ui(recording, |ui| {
                if ui.button("Стоп").clicked() {
                    self.stop_recording();
                }
            });

            // «Буфер» mirrors the facade's own replay state. On a live session it
            // toggles that state; with no session it boots a capture-only one, so
            // the button is useful before any «Запись». With no session and a zero
            // slider there is nothing to pre-roll, so it stays disabled.
            let replay_on = self.recorder.as_ref().is_some_and(Recorder::replay_enabled);
            let buffer_enabled = live_session || self.replay_seconds > 0;
            ui.add_enabled_ui(buffer_enabled, |ui| {
                let label = if replay_on {
                    "Буфер: включён"
                } else {
                    "Буфер: выключен"
                };
                if ui.button(label).clicked() {
                    self.toggle_replay();
                }
            });
        });

        ui.label(self.status_line());

        if live_session {
            ui.small(REPLAY_RING_HINT);
        } else if self.recorder.is_some() {
            ui.small(RELEASED_SESSION_HINT);
        } else if self.replay_seconds == 0 {
            ui.small(REPLAY_ZERO_HINT);
        } else {
            ui.small(BUFFER_NO_SESSION_HINT);
        }

        #[cfg(not(windows))]
        if self.recorder.is_some() {
            ui.small(LINUX_NO_CAPTURE_HINT);
        }
    }
}

impl eframe::App for CaptureDemo {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        // Poll the acks before drawing, so a transition that just arrived is drawn
        // in this same frame. Both polls drain the event channel without waiting.
        self.poll_acks();
        // The buffer switch-off release is not an ack for any command, so it
        // needs its own poll; it reads nothing while an ack is pending.
        self.poll_session_release();

        CentralPanel::default().show(ui, |ui| {
            ui.heading("video-capture demo");
            self.parameters(ui);
            self.actions(ui);
        });

        // The capture thread reads queued commands and emits acks only when a
        // frame arrives, so the UI must keep repainting: on a static desktop
        // egui's default update mode delivers few frames, and an ack would never
        // be polled. The request is delayed (`REPAINT_INTERVAL`) instead of
        // immediate: the ack polls above run inside this same pass, so an ack
        // that arrives between passes is drawn at most one interval later
        // (≤ 200 ms) rather than the moment it arrives — nothing is lost, only
        // drawn later. A delayed request buys exactly one repaint, so it is
        // re-issued every pass to keep the loop alive; a button press or slider
        // drag repaints immediately, because that immediate request beats the
        // pending 200 ms in egui's strict `<`.
        ui.ctx().request_repaint_after(REPAINT_INTERVAL);
    }
}

/// Window close releases the recorder explicitly so the capture thread is
/// joined and what it reported on the way out is logged rather than swallowed.
impl Drop for CaptureDemo {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(mut recorder) = self.recorder.take()
            && let Err(error) = recorder.shutdown()
        {
            tracing::warn!(%error, "recorder shutdown on window close failed");
        }

        // Linux has no capture thread to join; dropping the recorder is enough,
        // and calling `shutdown` there would only report UnsupportedPlatform.
        #[cfg(not(windows))]
        {
            self.recorder = None;
        }
    }
}

/// stdout for INFO+, stderr for WARN+ — one registry, two format layers.
/// Startup init: a subscriber that cannot be installed means another one is
/// already global, which this demo cannot undo, so it reports and continues.
fn init_tracing() {
    use tracing_subscriber::layer::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(io::stderr)
                .with_filter(tracing_subscriber::EnvFilter::new("warn")),
        );

    if let Err(error) = subscriber.try_init() {
        eprintln!("capture demo: tracing subscriber not installed: {error}");
    }
}

fn main() -> eframe::Result {
    init_tracing();
    tracing::info!("capture demo starting");

    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("video-capture demo")
            .with_inner_size([420.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "video-capture demo",
        options,
        Box::new(|_cc| Ok(Box::new(CaptureDemo::default()))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// eframe's invisible/minimized-window floor (`run.rs:27`, applied
    /// `run.rs:376-382`): a shorter interval would be silently raised for a
    /// hidden window, so a throttle below it buys nothing.
    const INVISIBLE_WINDOW_FLOOR: Duration = Duration::from_millis(100);

    /// The repaint cadence comes from the named const, and the literal exists
    /// only there: 200 ms, matching the shipped throttle.
    #[test]
    fn repaint_interval_throttled_pass_pins_repo_200ms() {
        assert_eq!(REPAINT_INTERVAL, Duration::from_millis(200));
    }

    /// The demo keeps repainting while the window is hidden (ack polling needs
    /// passes), so the interval must sit at or above eframe's floor to be
    /// effective rather than silently raised.
    #[test]
    fn repaint_interval_hidden_window_stays_above_eframe_floor() {
        assert!(REPAINT_INTERVAL >= INVISIBLE_WINDOW_FLOOR);
    }
}
