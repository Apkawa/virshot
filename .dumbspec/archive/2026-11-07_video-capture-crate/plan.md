# 2026-11-07_video-capture-crate: implementation plan (isolated video-capture crate)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan
- [x] research `windows-capture` 2.0.1 (API, encoders, codecs, rate control, replay, audio, deps, cross-build)
- [x] spec v1 with feasibility table (not-supported items marked honestly)
- [x] plan

**Criterion:** research.md answers all 11 questions with evidence; spec.md v1 exists with Created/Changes; open questions = none.
**Commit:** `docs(dumbspec): video-capture research + spec v1 + plan` ✅ `82706a9`

## Stage 1 — Crate skeleton + config types (platform-independent logic)
- [x] Create `crates/video-capture/`: Cargo.toml (`#![forbid(unsafe_code)]` lib, `windows-capture = "2.0.1"` under `[target.'cfg(windows)'.dependencies]`, path-deps `image`/`thiserror`/`tracing`), workspace member.
- [x] TDD: `config.rs` — CaptureConfig enums (EncoderKind: Auto + Nvenc/Amf/X264 marked library-unsupported, Codec: H264/Hevc, BitrateMode: Cbr + Cqp/Crf marked unsupported, Resolution: Seven20p/One080p/Full, Fps: Thirty/Sixty, AudioMode: None/System/Mic/Both with system/mic marked unsupported), defaults (1080p, 60fps, cbr 15 Mbps, hevc... per spec default codec h264), target size selection for Full = native, replay clamp 0..600, fps pacing interval.
- [x] TDD: `ring.rs` — bounded ring buffer (duration seconds + byte cap), push evicts oldest, snapshot order, empty when replay=0.
- [x] `error.rs` — thiserror enum; Linux stub entry (UnsupportedPlatform).
- [x] Gates: `./scripts/check.sh` green (Linux, tests run), `./cargo.sh clippy --workspace --all-targets -D warnings`.

**Criterion:** `cargo test -p video-capture` passes on Linux (config/ring unit tests); `cargo check --target x86_64-pc-windows-gnu -p video-capture` compiles with windows-capture linked. ✅ independent acceptance PASS (`./.tmp/acceptance-stage1.log`).
**Commit:** `feat(video-capture): crate skeleton with config and ring buffer types` ✅ `dbc643c` — landed as ONE commit (hook `git add -u .` swept the incidental i18n clippy fix into it; body documents the deviation; the separate `fix(tests):` never landed)

## Stage 2 — Windows capture engine (behind cfg(windows))
- [x] `engine.rs`: GraphicsCaptureApiHandler over `Monitor::primary()`, always-on capture thread; ring feed when replay>0; commands via channel (Start(path)/Stop/Quit); Start → VideoEncoder (settings from CaptureConfig: codec subtype H264 default explicit, bitrate, frame_rate), replay via `send_frame_buffer`, live frames via `send_frame` (Full) or CPU-resize into `send_frame_buffer` (720p/1080p); Stop → `finish()` + keep capturing for ring.
- [x] facade `Recorder`: spawn thread, `start(path)/stop()`, graceful shutdown; no unsafe, no unwrap in loop.
- [x] `./cargo.sh check --target x86_64-pc-windows-gnu` + `./cargo.sh clippy --target x86_64-pc-windows-gnu -p video-capture --all-targets -D warnings`.

**Criterion:** engine + facade compile clean for Windows-gnu target; Linux `cargo check` unaffected; runtime acceptance deferred to user's manual Windows run. ✅ independent acceptance PASS (pre-existing root locale flakiness noted, green under --test-threads=1).
**Commit:** `feat(video-capture): windows capture engine with replay ring` ✅ `e716bb2`

## Stage 3 — egui test example
- [x] `examples/capture_demo.rs` (dev-deps: eframe/egui/tracing-subscriber — existing approved stack): path field (default `%USERPROFILE%\Videos\test.mp4`), all parameters (encoder — disabled/selectable with “MF auto” hint; codec h264/hevc; bitrate mode — CBR active, CQP/CRF disabled with “not supported by library” hint; bitrate slider; resolution 720p/1080p/full; fps 30/60; replay_buffer slider 0..600 + memory estimate; audio none active, system/mic/both disabled with hint), запись/стоп button, status line, tracing subscriber on stdout/stderr.
- [x] Linux: `cargo check -p video-capture --all-targets` must pass (example behind cfg(windows) runtime guard, UI code compiles everywhere).
- [x] Windows build: `./cargo.sh build --target x86_64-pc-windows-gnu --example capture_demo`.

**Criterion:** example builds for x86_64-pc-windows-gnu; UI exposes all spec parameters with honest disabled states; logs go to stdout/stderr. Manual Windows check by user deferred. ✅ independent acceptance PASS (capture_demo.exe built; xvfb smoke alive; lock diff = 0 new packages).
**Commit:** `feat(video-capture): egui capture demo example`

## Stage 4 — Workspace gates + docs
- [x] `./scripts/check.sh` (root + crate tests), `./cargo.sh test --workspace`, `./cargo.sh fmt` + `fmt --check` at boundary.
- [x] Windows artifacts: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` (workspace).
- [x] README.md for the crate: parameters table incl. “not implemented” items, how to run the demo on Windows.

**Criterion:** all gates green (Linux tests + clippy -D warnings; Windows-gnu compile of lib+example+tests); README documents implemented vs not-possible parameters. ✅ all gates exit 0 (ws test, ws clippy, windows-gnu test --no-run, check.sh); README reviewed + fixed (API example default codec → H264; honest memory bound ~139/278 GiB replaces spec's ~5 GB estimate).
**Commit:** `chore(video-capture): workspace gates and crate README`

## Post-goal fixes (user's manual Windows runs)
- [x] MinUpdateInterval unsupported on the user's build → probe + `Default` fallback with a warning (`3d7734d`, README `32efee0`); every resolved parameter logged to the console at engine creation and at recording start.
- [x] Upside-down 720p recording → row flip in the buffer feed path (`b5414be`); orientation proven by debug dumps (`a95a002`), user confirmed upright.
- [x] Unreadable text + jerky frames → proportional target width (height preset, width from desktop aspect) and selectable scale filter (`9ad6118`): own Nearest/Bilinear kernels (4.6 / 10.1 ms per frame) plus image CatmullRom/Gaussian/Lanczos3 (148–207 ms, out of the 30 fps budget), measured on 1680x1050 → 1152x720.

## Stage 5 — Replay-buffer toggle, RAM saving, recording-start pause fix (spec v2)
- [x] `ring.rs`: `ReplayRing::set_capacity(max_frames)` — raising capacity keeps the
  already-buffered frames (oldest-eviction applies), `set_capacity(0)` drains and frees
  the memory. ✅ 4 TDD tests added (drain, shrink evicts oldest, growth keeps frames,
  reenable after disable); `./cargo.sh test -p video-capture` = 75 passed; clippy clean.
- [x] `engine.rs`: `EngineCommand::SetReplay(u32)` — applied on the next captured frame
  (enable → rebuild capacity clamped by fps; disable → drain + capacity 0); the ring is
  not fed while recording is active (`encoder.is_some()`) — RAM saving, the pre-roll
  snapshot stays intact. ✅ handler `set_replay()` + `ring_wanted` guard; Linux test +
  clippy and windows-gnu check + clippy (`-D warnings`) all exit 0.
- [x] `engine.rs`: start-pause fix — pacing `last_timestamp` is not advanced while no
  encoder exists, so the first frame after the encoder is always sent (frames arriving
  before the encoder must not consume the pacing budget). ✅ pacing gate + clock update
  now apply only while an encoder exists (Linux/clippy + windows-gnu check/clippy exit 0).
- [x] `engine.rs`: reset `last_timestamp` to `None` in `start_recording` so a Stop→Start
  inside one frame interval still sends the first frame of the new recording. ✅ placed
  after the pre-roll replay loop, before `self.encoder = Some(encoder)`; all four gates
  exit 0.
- [x] `engine.rs` facade (`Recorder`): `enable_replay(seconds)` / `disable_replay()` /
  `replay_enabled()` sending `EngineCommand::SetReplay`; Linux stub → `UnsupportedPlatform`;
  docs: applied on the next captured frame. ✅ requested state kept locally (no roundtrip);
  3 Linux stub tests added (75 → 78 passed), Linux clippy + windows-gnu check/clippy exit 0.
- [x] `engine.rs` facade (`Recorder`): non-blocking ack path — `start(path)` / `stop()`
  queue the command and return once queued; `poll_recording_ack()` / `poll_stop_ack()`
  report the outcome on the caller's loop (the UI no longer freezes on encoder init /
  mux flush); boot helper `Recorder::recording(config, path)` pre-enqueues
  `EngineCommand::Start` BEFORE `start_free_threaded`, so the first frame after
  `StartCapture` handles Start. Linux stub → `UnsupportedPlatform`. ✅ `AckState` +
  `ack_outcome` (time injected, 8 new tests, 86 passed), `await_ack` deleted, lib.rs crate
  doc updated; Linux clippy + windows-gnu check/clippy exit 0; demo still compiles.
- [x] `capture_demo.rs`: «Буфер» toggle button next to «Запись»/«Стоп»; status line
  distinguishes "capture session open" from "replay armed" (no "replay armed" at 0);
  ack polling in the eframe logic pass instead of blocking the UI; continuous
  `request_repaint` so a static desktop still delivers frames; memory-estimate label
  clarifies it is computed from the native-desktop hint. ✅ `Status` state machine with
  `StartPending`/`StopPending`, `Recorder::recording` boot (debug-dump path keeps
  `with_debug_dump` + queued `start`), «Буфер: включён/выключен», honest Russian status
  lines, `poll_acks()` before drawing + `request_repaint`; all five gates exit 0, xvfb
  smoke run alive (exit 124 = timeout kill, no panic).
- [x] `README.md`: section on enable/disable of the buffer; row "compressed stream ring —
  not implementable with the library" with evidence (CreateUncompressed source descriptor,
  no packet output API, InMemoryRandomAccessStream without truncation); RAM table for
  1680x1050; API snippet updated for the new facade methods. ✅ README 227 → 344 lines
  (+139/−22): new «Кольцевой буфер: включение/выключение в рантайме» section, RAM table
  (exact bytes + GiB, decimal GB noted), feasibility row for the compressed ring, rewritten
  buttons/status bullets + API snippet, indicator bullet; test/clippy/windows-gnu gates exit 0.
- [x] Gates: `./scripts/check.sh`, `./cargo.sh test --workspace`, `./cargo.sh test --no-run
  --target x86_64-pc-windows-gnu -p video-capture --all-targets`, `./cargo.sh clippy --target
  x86_64-pc-windows-gnu -p video-capture --all-targets -- -D warnings`, `./cargo.sh fmt` +
  `fmt --check` — all exit 0; commit `f5572f7`; release demo exe rebuilt
  (`capture_demo.exe`, 7 640 064 Б, md5 `21edcf6b41088d680460c2c3ddd0552f`).

**Criterion:** `cargo test -p video-capture` green (new ring capacity/drain tests);
windows-gnu compiles lib+example+tests without warnings; the UI has the buffer toggle
and an honest status line; README marks the compressed ring not possible; Start is
handled by the first frame (pre-queue + pacing guard visible in the code).
**Commit:** `feat(video-capture): runtime replay toggle, RAM saving, faster recording start`

## Stage 6 — Pre-roll contract, buffer-only session, session release at replay 0 (spec v3)

- [x] `engine.rs` pure predicate (TDD red → green): `session_should_release_after_stop`
  — true only when the ring holds nothing (capacity 0); tests in `engine.tests.rs`
  (`session_should_release_after_stop_*`) run on Linux. ✅ engine.rs:570-580 + 2 tests
  (engine.tests.rs:244-258); red = E0425 ×4; green = 88 passed, clippy `-D warnings` exit 0.
  Deviation: `#[cfg_attr(not(windows), allow(dead_code))]` until the Windows stop-path
  consumes it (same convention as `AckState`).
- [x] `engine.rs` (Windows): new `EngineEvent::CaptureReleased`; `stop_recording` sets a
  release flag when the predicate says the session has no reason to stay open; the next
  frame handles the flag → `capture_control.stop()` + `CaptureReleased`. Ring refill after
  Stop stays unchanged (fills again while capacity > 0). ✅ engine.rs:223-227 (variant),
  :694-699 + :764 (flag), :1114-1122 (arm after the ack), :798-807 (release on next frame);
  ack always precedes the release event. Deviation: one exhaustive arm added in
  `Recorder::shutdown`'s `find_map` (:1702-1705, `CaptureReleased => None`). 88 tests +
  Linux/windows-gnu check+clippy exit 0.
- [x] `engine.rs` facade: released-session state — polls drain `CaptureReleased` and mark
  the recorder released; `start` / `stop` / `enable_replay` / `disable_replay` on a released
  recorder return `CaptureError::NotConfigured`; Linux stubs mirror the shape + tests. ✅
  `event_is_capture_release` (:597, TDD red→green), `Recorder::session_released` (:1244,
  false in every construction path), `drain_events`/`poll_pending` take `released: &mut bool`
  (:1267-1290, :1308-1324) — release never consumes an ack; guards in start/stop/enable_replay
  on both platforms (:1557, :1584, :1602, :1625, :1710, :1736); `disable_replay` inherits via
  delegation. 6 new tests → **94 passed**, Linux + windows-gnu check/clippy exit 0.
- [x] `examples/capture_demo.rs`: «Буфер» without a session boots a capture-only session
  (`Recorder::new(cfg)` + `enable_replay(slider seconds)`) so the ring pre-rolls before any
  recording; «Запись» reuses a live session through queued `start(path)` instead of building
  a new `Recorder`; status lines say «буфер снова наполняется» after Stop and
  «захват освобождён» when replay is 0. ✅ demo +222/−38: `Capturing { last_file, released }`,
  `start_capture_session()` (`Recorder::new`, ring armed by config; slider 0 = no session),
  `live_session()`, reuse branch `recorder.start(path)` with `NotConfigured` → rebuild,
  `on_stop_ack` sets `released = !replay_enabled()` and keeps the recorder alive (verified:
  windows-capture `InternalCaptureControl::stop()` only sets an AtomicBool, `cleanup()` uses
  `Option::take()` → second stop cannot error); Russian hints `REPLAY_ZERO_HINT`,
  `RELEASED_SESSION_HINT`, `LINUX_NO_CAPTURE_HINT`. 94 tests, Linux + windows-gnu gates exit 0,
  xvfb smoke alive (exit 124, no panic).
- [x] `README.md`: pre-roll contract (file ≥ N seconds, includes the N seconds before the
  button; honest caveat that a sparse delivery window is shorter), buffer enabled
  independently of recording, ring resumes after Stop, zero-buffer Stop releases the capture
  session (indicator disappearing is expected OS behavior, not fixed). ✅ +121/−29
  (344 → 436 lines): new subsections «Пре-ролл как контракт записи», «Буфер отдельно от
  записи», «Наполнение кольца: во время записи и после „Стоп“», «Буфер 0 освобождает захват»;
  API snippet leads with `Recorder::new(config)` + reuse `start(path)` + `NotConfigured`;
  WGC bullet keeps indicator-stays behavior + v3 exception; RAM table and compressed-ring
  «не получается» row preserved.
- [x] Gates: `./scripts/check.sh`, `./cargo.sh test --workspace`, windows-gnu
  `test --no-run` + `clippy -D warnings`, `fmt` + `fmt --check`; commit; rebuild release demo exe.
  ✅ `check.sh` exit 0 (410 + 9 e2e passed, 1 ignored), `test --workspace` exit 0 with
  `video-capture` **94 passed / 0 failed**, windows-gnu `test --no-run --all-targets` exit 0
  (lib + example `.exe` built), windows-gnu `clippy --all-targets -- -D warnings` exit 0,
  `fmt` + `fmt --check` exit 0. Commit `89419f1` — `feat(video-capture): buffer-only session,
  capture release on zero replay` (6 files, +752/−82, hook gate passed). Release exe rebuilt:
  `target/x86_64-pc-windows-gnu/release/examples/capture_demo.exe` — 7 648 256 Б,
  md5 `fed2f4406fd5b474f6823c458b35bcca`.

**Criterion:** `cargo test -p video-capture` green including the release-predicate and
released-session facade tests; windows-gnu compiles lib+example+tests with no warnings; the
code shows the ring snapshot fed into the encoder at Start and the release-on-zero path; the
demo can run the buffer without a recording and releases the capture after Stop at replay 0;
README documents all four semantics honestly.
**Commit:** `feat(video-capture): buffer-only session, capture release on zero replay`

## Stage 7 — Async pre-roll writer thread, release on buffer-off (spec v4)

- [x] Research (windows-capture 2.0.1 source): confirm `VideoEncoder: Send + !Sync`, what
  `send_frame_buffer` / `finish` do under the hood (blocking vs async submit), whether the
  capture frame callback is the only place that can feed today, and copy cost of
  `frame.buffer()`; verdict: writer-thread ownership move is compile-legal on
  `x86_64-pc-windows-gnu`.
  ✅ `./.tmp/stage7-research.md` + probe crate `./.tmp/stage7-probe/`: `VideoEncoder` has an
  explicit `unsafe impl Send` (encoder.rs:1369-1370) and provably is **not** `Sync` (E0277 via
  `IUnknown(NonNull<c_void>)` → `IDirect3DSurface` → `CachedSurface`), so exactly one thread may
  own it; `send_frame_buffer` is non-blocking (unbounded internal mpsc, :1272), `finish()`
  blocks (EOS + join of the transcode thread, :1312-1341), `VideoEncoder::new` blocks ~1 s
  (three `.join()`, :942-951). WinRT init is per-thread and `winrt::WinRT` is `pub(crate)`, so
  the encoder must be **created on the capture thread and moved**; `Frame<'a>` is not `Send`,
  so the GPU surface cannot leave the callback, and the frame pool is single-buffered
  (graphics_capture_api.rs:179). The probe compiles on `x86_64-pc-windows-gnu`.
- [x] `engine.rs` pure predicate (TDD): generalize the release decision — release when the
  ring holds nothing **and** no recording is active (`session_should_release`), covering
  both Stop-with-zero-capacity and `SetReplay(0)` while idle; tests on Linux.
  ✅ `session_should_release(ring_capacity, recording_active)` (engine.rs:628-630) +
  `event_is_capture_release` (:634); four TDD tests in `engine.tests.rs:244-280` (zero capacity
  idle → release, zero capacity recording → stays open, non-zero idle → stays open, non-zero
  recording → stays open). Linux suite: **98 passed, 0 failed**.
- [x] `engine.rs` (Windows): encoder writer thread — `Start` moves the `VideoEncoder` +
  ring snapshot into a dedicated thread that feeds pre-roll then live frames from a bounded
  queue; `on_frame_arrived` during recording only copies the BGRA buffer and pushes; acks
  (`RecordingStarted` / `RecordingStopped` / `CommandFailed`) come from the writer via the
  same event channel; `SetReplay(0)` while idle arms the release flag (indicator off on the
  next frame).
  ✅ `LIVE_QUEUE_FRAMES = 8` (:741), `EncoderWriter { live_tx, thread }` (:752),
  `run_encoder_writer` (:776-838) — pre-roll first, FIFO live frames, `encoder.finish()` (:815)
  then `RecordingStopped` (:818), one `CommandFailed` (:829-833), done-notice (:835-837).
  `start_recording` (:1247-1318) creates the encoder on the capture thread (:1262), snapshots
  the ring, bumps the generation, spawns the writer; the callback only copies BGRA +
  `try_send` (:1046-1114) — `Ok` → `RecordingStarted` on the first queued live frame, `Full` →
  warn + drop, `Disconnected` → detach + failure + `is_finished()` diagnostic. `stop_recording`
  (:1350-1360) only detaches the handle; the done-notice poll (:986-999) arms
  `session_should_release(capacity, false)` so `CaptureReleased` lands after muxing, and
  `set_replay(0)` while idle arms it on the frame that read the command (:1401). Proof:
  `send_frame_buffer` appears **only** inside the writer body (:788, :800) and `finish()` only
  at :815 — nothing in `on_frame_arrived`.
- [x] `engine.rs` facade + Linux stubs: ack plumbing unchanged (polls drain writer events);
  released-session guards already cover `disable_replay`; keep `replay_enabled()` semantics.
  ✅ Ack polls unchanged (`poll_recording_ack`/`poll_stop_ack` drain the same channel; a
  `CaptureReleased` never consumes a pending ack, engine.rs:2040). Added
  `Recorder::poll_session_release()` (:1947-1959, drains only while no ack is pending) + Linux
  stub (:1963-1968), with two stub tests (`facade_poll_session_release_*`); release never
  changes `replay_enabled()`.
- [x] `examples/capture_demo.rs`: «Буфер» off without a recording → status
  «захват освобождён», next «Буфер»/«Запись» boots a fresh session; buffer-on boots the
  ring session as today; no blocking anywhere.
  ✅ `CaptureDemo::poll_session_release` (:597-631) called next to the ack polls in `ui`
  (:1031-1034) — non-blocking, only reads when no ack is pending; `on_stop_ack` prefers the real
  signal (`poll_session_release() || !replay_enabled()`, :674-681); hint covers both triggers
  (:94-98); `live_session()` (:484-487) treats a released session as dead, so «Буфер»/«Запись»
  rebuild via `start_capture_session()` (:446-469) / `Recorder::start` fallback on
  `NotConfigured` (:372). Release wording «захват освобождён (буфер выключен)» (:805).
- [x] `README.md`: async pre-roll (no freeze at «Запись»), release when the buffer is
  switched off, and the compressed-buffer row stays «не получается» + RAM table.
  ✅ New «Писатель энкодера: пре-ролл без фриза» (:297-329), rewritten «Буфер 0 освобождает
  захват» (:348-380), RAM honesty (:197-204 — 10 s @30 fps `720p` ≈ 1.03 GiB RAM vs ≈ 18 MiB
  mp4 @15 000 kbps), «replay в сжатом виде» row stays «не получается», risks added for the ~1 s
  encoder creation on the capture thread, lagging writer (dropped frames), and «Стоп» not
  waiting for muxing; stale claims (dump gate, GPU fast path, mux-by-capture-thread) fixed.
- [x] Gates: Linux `check.sh` + `test --workspace`, windows-gnu `check`/`test --no-run`/
  `clippy -D warnings`, `fmt --check`; commit; rebuild release demo exe.
  ✅ `./scripts/check.sh` (with e2e, 9 passed) exit 0; `./cargo.sh test --workspace` green on
  retry (first run had the known rust_i18n locale-order flake in two root toast tests, pass in
  isolation); `./cargo.sh test -p video-capture` 98 passed; windows-gnu `check`,
  `test --no-run`, `clippy --all-targets -D warnings` all exit 0; commit below; release exe
  rebuilt with a new md5.

**Criterion:** the recording path never calls `send_frame_buffer` inside the frame callback
(pre-roll and live frames are fed by the writer thread); disabling the replay buffer with no
active recording releases the capture session (`CaptureReleased` + `NotConfigured` afterwards);
(94+ Linux tests green → 98 shipped), windows-gnu lib+example+tests compile with `-D warnings`; README states
all of it plus the still-unimplementable compressed ring.
**Commit:** `fix(video-capture): async encoder writer, release capture when buffer off`

## Stage 8 — Public API rework: u16 resolution, Bilinear filter, u8 fps, drop dead options (spec v5)

- [x] Audit the public API surface and blast radius: which config fields reach the library
  (honored) vs are markers only (deletable), where `Triangle` comes from, whether an arbitrary
  `u16` height is blocked by anything, and every consumer (engine, demo, README, tests).
  ✅ `./.tmp/stage8-api-audit.md`: honored = `codec` (H264/HEVC sub_type), `bitrate_kbps`
  (`bitrate` → `MF_MT_AVG_BITRATE`), `resolution` (`target_size` → `VideoSettingsBuilder`),
  `fps`, `scale_filter` (CPU `resize_bgra` only — the library has no resize/filter API);
  markers = `EncoderKind` (not even a `CaptureConfig` field, library has no encoder selection)
  and `BitrateMode` (never passed; CBR only, no `ICodecAPI`); `AudioMode`/`audio` is a stub kept
  per the user. `Triangle` is our own variant name mirroring `image::FilterType::Triangle`
  (implemented as separable bilinear; `label()` already says «Bilinear»). Library dims are all
  `u32`, so `u16` heights pass; zero height is unusable (resize/flip yield empty bytes, no 0×0
  texture), width is derived from aspect, target fixed at recorder construction.
- [x] `config.rs` pure logic (TDD): `Resolution::Height(u16)` target computation — width derived
  from aspect (floor, ≥1), heights above native stay native (no upscale), height `0` invalid;
  tests on Linux for the 160/720/1080/native/0 cases.
  ✅ RED first (`./cargo.sh test -p video-capture` exit 101 — `Height` variant / `label` missing,
  `./.tmp/stage8-resolution-red.log`), GREEN 109 passed (+11 config tests,
  `./.tmp/stage8-resolution-green.log`). API: `enum Resolution { Height(u16), Full }` (:76-87),
  associated consts `P720`/`P1080` (:90/:94 — existing call sites keep compiling),
  `target_size` (:107, `Height(0)` reports native instead of panicking), private
  `valid_target_size` (:115 — `None` only for height 0, promote to `pub(crate)` with engine
  validation), `label()` (:125 — `"160p"`, `"Full"`), manual `Default` = 1080p (:133). Exact
  widths: `Height(160)` on 1920×1080 → (284,160) (not 170 — 160·16/9 = 284.44, floor 284),
  21:9 2560×1080 → (379,160)/(1706,720), `Height(2000)` stays native, `Height(720)` on (1,1080)
  → (1,720). Touched minimally: `engine.tests.rs:299` dump now `resolution=Height(720)`,
  `capture_demo.rs:189-196` label delegates, `lib.rs:56-61` doc reworded. Gates exit 0: Linux
  check/clippy/test (109), windows-gnu check/clippy (`./.tmp/stage8-resolution-gates-final.log`).
- [x] `config.rs` API: `Resolution::Height(u16) | Full` (720/1080 become `Height` consts),
  `ScaleFilter::Triangle` → `Bilinear`, `fps: u8`, delete `EncoderKind` and `BitrateMode`
  (+ their methods); keep `codec`, `bitrate_kbps`, `replay_seconds`, `audio` (stub, user decides).
  ✅ RED (exit 101, 21 errors — `fps: 30` expected `Fps`, `invalid_reason` missing,
  `./.tmp/stage8_red.log`) → GREEN 117 passed (`./.tmp/stage8_green_test.log`). `ScaleFilter`
  variants `Nearest | Bilinear(default) | CatmullRom | Gaussian | Lanczos3` (:109/:116),
  `CaptureConfig { codec, bitrate_kbps, resolution, scale_filter, fps: u8 (:183), replay_seconds,
  audio }` (:172), manual `Default` (:189-199, fps 30), `bitrate_bps()` (:214),
  `frame_interval()` moved to `CaptureConfig` (:222, integer ns — 30 → 33 333 333 ns, no drift).
  Deleted: `EncoderKind`/`is_library_selectable`, `BitrateMode`/`is_supported`/`label` + field,
  `enum Fps`, their 5 tests, demo encoder/bitrate-mode controls, `config_dump` mode line,
  `lib.rs` bullets. `rg 'EncoderKind|BitrateMode|Triangle|enum Fps' crates/video-capture` →
  only README (deferred docs unit). Kept: `bitrate_kbps` (CBR via `MF_MT_AVG_BITRATE`), `Codec`,
  `replay_seconds`, `AudioMode` stub, all 5 filter kernels (behavior unchanged).
- [x] `engine.rs`: validation (`Height(0)` → `CaptureError::NotConfigured` in `new`/`recording`),
  `frame_rate(u32)` from `u8` fps, `config_dump` without the deleted options, resize dispatch and
  docs renamed to `Bilinear`, tests updated (Linux + Windows cfg paths).
  ✅ `reject_invalid_config` (:362, logs the Russian reason, payload-less `NotConfigured` so
  existing match arms stay valid) wired at :1699 (covers `new`/`with_debug_dump`/`recording`) and
  Linux :1795/:1818/:1833; messages: «частота кадров 0 недопустима: нужен fps от 1»,
  «высота разрешения 0 недопустима: нужна высота от 1 пикселя». fps sites now `u8`:
  `interval_ticks` :611, `frames_for` :617, dump :671, `Custom(frame_interval())` :701, log :926,
  `.frame_rate(u32::from(…))` :1349, RAM estimate :1437 — pacing logic untouched. Resize dispatch
  `Bilinear` :409, `bilinear_bgra` doc :446, dead `image_resize` arm :549. `config_dump` :664 no
  longer prints a bitrate mode (test asserts the substring is absent). 5 new Linux tests:
  refusal on zero fps/zero height in all three constructors + accept of a custom height.
- [x] `examples/capture_demo.rs`: resolution = numeric height (`u16`) + «Full», fps = numeric
  (`u8`), encoder/bitrate-mode controls and hints deleted, filter labels/hints renamed, RAM
  estimate follows the new types.
  ✅ Resolution ComboBox gone; UI :896-908 — checkbox «Полный размер экрана (Full)» (numeric field
  hidden when checked) + row «Высота кадра (px)» `DragValue u16 1..=4320` (consts :72-73, default
  1080 :77); state `full: bool` + `height: u16` (:274/:277), `fn resolution()` (:327-333) feeds
  `config()` (:342) and the RAM estimate (:938); hints :102/:106 state the honest rules (width from
  aspect — 160 → 284×160, no upscale above native, height ≥1, target fixed at recorder creation so
  the new height applies to the next «Запись»/«Буфер»). Estimate keeps `target_size(NATIVE_HINT)`
  (1920×1080 is a named hint, demo has no native accessor — no fabricated numbers, heights above
  native collapse to native). Encoder/bitrate-mode widgets deleted earlier in this stage; fps
  `DragValue u8 1..=60` (:873) with the Russian hint (:83); filter labels/hints say `Bilinear`
  (:204/:860). Gates exit 0 (Linux check/clippy/test 117, windows-gnu check/clippy), no exe built.
- [x] `README.md`: parameter table (arbitrary height + `Full`, `u8` fps, «Bilinear», encoder and
  bitrate-mode rows removed — marked deleted, not «не получается»), API snippet, scaling section.
  ✅ Docs-only (`crates/video-capture/README.md` was the only file touched). Table: encoder and
  bitrate-mode rows gone, resolution row = произвольная высота `u16` ≥1 (`Height(160)`, `P720`/
  `P1080` = `Height(720)`/`Height(1080`), `Full` = native), fps row `u8` 1..60 default 30; post-table
  note says non-working options were **removed** from the API (spec v5), «replay в сжатом виде» row
  untouched. Scaling section now derives widths (160 → 284×160 on 1920×1080, `Height(1080)` on
  1680×1050 stays native). Memory numbers recomputed from `w*h*4*fps*seconds`: 10 s @30 fps =
  300 frames — `Height(720)` 1280×720 → 1 105 920 000 Б ≈ 1.03 GiB (1152×720 on 1680×1050 →
  995 328 000 Б ≈ 0.93 GiB), `Full` 1920×1080 → 2 488 320 000 Б ≈ 2.32 GiB (native 1680×1050 →
  1.97 GiB), `Height(160)` 284×160 → 54 528 000 Б ≈ 52.0 MiB, mp4 @15 000 kbps → 18 750 000 Б
  ≈ 17.9 MiB; sub-table rows verified against `ring::estimated_memory_bytes`. Limits section adds
  «выбор энкодера» and «CQP/CRF» as удалено из API + validation messages + typed `NotConfigured`;
  API snippet rewritten for the new types and compile-checked in `./.tmp/snippet-check`. Only
  remaining `bitrate_mode` match in the crate is the negative assertion in `engine.tests.rs:318`
  (asserts the dump does NOT contain it), and «Triangle» is gone everywhere.
- [x] Gates + commit + rebuild the release demo exe.
  ✅ `./scripts/check.sh` (with e2e) exit 0 (`./.tmp/stage8_gate_checksh.log`); `./cargo.sh
  test --workspace` exit 0 (410 passed root + 9 e2e, no locale flake this run);
  `./cargo.sh test -p video-capture` 117 passed; windows-gnu `check`, `test --no-run`
  (lib + example test binaries), `clippy --all-targets -D warnings` all exit 0. Commit
  `41cab32 refactor(video-capture): u16 resolution, u8 fps, bilinear filter, drop encoder and
  bitrate-mode options` (8 files, +773/−405, hook chain green). Release demo exe rebuilt:
  `target/x86_64-pc-windows-gnu/release/examples/capture_demo.exe`, 7 671 808 Б,
  md5 `b691548e3ea298d937089a70e73dccdf` (was `ab93afc1…`).

**Criterion:** the crate exposes `Resolution::Height(u16) | Full` (arbitrary height, width from
aspect, no upscale, `Height(0)` refused with `NotConfigured`), `fps: u8`, and
`ScaleFilter::Bilinear` — `EncoderKind`, `BitrateMode`, `enum Fps`, and `Triangle` are gone from
sources, tests, demo, and README (the single remaining `bitrate_mode` substring is the negative
assertion in `engine.tests.rs:318`, which proves the dump no longer prints it); Linux tests green
(117), windows-gnu lib+example+tests compile with `-D warnings`, README/demo expose exactly those
controls.
**Commit:** `refactor(video-capture): u16 resolution, u8 fps, bilinear filter, drop encoder and bitrate-mode options`

## Deferred (user-side)
- Manual Windows acceptance of the demo (recording, replay pre-roll, MP4 playback).
- Audio system/mic capture — needs a new approved dependency (cpal) or unsafe shim; marked not implemented.
- Windows screen-capture indicator (yellow border) — normal OS behavior while the capture
  session is open; explicitly not fixed per the user (2026-11-07).
- Compressed-packet replay ring (encoded H.264/HEVC frames in RAM instead of raw BGRA) —
  not implementable with `windows-capture`; marked in spec.md, no workaround.
