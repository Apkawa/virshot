# Research: `windows-capture` (crates.io) — input for `crates/video-capture`

Task date: 2026-11-07. Research only; nothing in the project was modified.

Evidence sources used:

- crates.io API: <https://crates.io/api/v1/crates/windows-capture>,
  <https://crates.io/api/v1/crates/windows-capture/2.0.1/dependencies>
- Upstream repo / README: <https://github.com/NiiightmareXD/windows-capture>,
  raw README fetched from `raw.githubusercontent.com/NiiightmareXD/windows-capture/main/README.md`
- Unpacked published crate (authoritative, greppable):
  `SRC = .cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/windows-capture-2.0.1`
  (also `windows-capture-1.5.0` and `windows-capture-1.0.12` fetched for history)
- Rendered docs: <https://docs.rs/windows-capture/latest/windows_capture/> and
  <https://docs.rs/windows-capture/latest/windows_capture/encoder/index.html>
- Compile checks in scratch crates (./.tmp/, `[workspace]` stub so they never join the
  project workspace):
  - Linux: `./.tmp/wc-check` → `./cargo.sh check` → **exit 101**, log `./.tmp/wc-linux-check.txt`
  - MinGW cross: `cargo check --target x86_64-pc-windows-gnu` → **exit 0**, log `./.tmp/wc-windows-gnu-check.txt`
  - Historical: `./.tmp/wc-check15` (`=1.5.0`), `./.tmp/wc-check10` (`=1.0.12`)

All line references below are `SRC/<file>:<line>` against the unpacked **2.0.1** source.

---

## 1. Name, version, license, description, native/runtime requirements, non-Windows build

**Identity** (crates.io `/api/v1/crates/windows-capture`):

- name: `windows-capture`; newest / `max_stable` / latest version: **2.0.1** (published 2026-08-08, author `NiiightmareXD`)
- license: **MIT** (`SRC/Cargo.toml.orig:10`, crates.io `license: "MIT"`)
- description: **"Fastest Windows Screen Capture Library For Rust 🔥"**
- repository: <https://github.com/NiiightmareXD/windows-capture>; docs: <https://docs.rs/windows-capture>
- edition **2024** (`SRC/Cargo.toml.orig:5`), `resolver = "3"` (`:19`), **no `rust-version`** key
- crates.io metadata: 3536 lines of Rust / 12 files, ~2.57 M downloads, `features: {}` →
  **the crate exposes no cargo features at all** (nothing to enable/disable)
- Version history relevant to us: 1.0.12 (no video encoder), 1.5.0 (encoder via Media
  Foundation), 2.0.1 ( DXGI duplication support + "enhanced video encoder … improved
  stability and monotonic audio timing" per README).

**Native / runtime requirements** (no vendored binaries, nothing to install at build time):

- Win32/WinRT bindings come from the `windows` crate 0.62.2, whose imports are emitted as
  `#[link(name = $library, kind = "raw-dylib", ...)]` — verified in
  `windows-link 0.2.1/src/lib.rs:5-31`. `raw-dylib` means **no import libraries are needed at
  link time**, so the project's MinGW-w64 cross-build works (empirically confirmed, exit 0).
  DLLs are resolved by the Windows loader at runtime: `d3d11.dll`, `dxgi.dll`, `user32.dll`,
  `kernel32.dll`, `combase.dll` (WinRT activation), Media Foundation (`mf.dll` / `mfplat.dll`
  / encoder MFTs). `windows-capture` itself never calls `LoadLibrary`/`libloading`:
  `rg -in 'LoadLibrary|libloading|dll' $SRC/src` → **0 matches**.
- Runtime API availability: Windows Graphics Capture (`Windows.Graphics.Capture`) needs
  Windows 10 1803+; the crate exposes capability probes instead of guessing —
  `GraphicsCaptureApi::is_supported()`, `is_cursor_settings_supported()`,
  `is_border_settings_supported()`, `is_secondary_windows_supported()`,
  `is_minimum_update_interval_supported()`, `is_dirty_region_supported()`
  (`SRC/src/graphics_capture_api.rs:431-474`). Video encoding needs Media Foundation
  transcoding (the `windows` feature `Media_Transcoding`, `SRC/Cargo.toml.orig:36`).
- No native build packages beyond MinGW-w64 (already configured in `.cargo/config.toml`).
  No GL/audio native libs.

**Non-Windows compilation**: NOT gated, and NOT compilable.

- `rg 'cfg\(' $SRC/src` → **0 matches**: there is no `cfg(windows)` gating anywhere in the
  crate (including `SRC/src/lib.rs:158-194`, which only has `#![warn(...)]` + module list).
- Linux `cargo check` fails during dependency compilation — `windows-future 0.3.2` does not
  build on Linux (16 errors, `./.tmp/wc-linux-check.txt`):
  `cannot find type IMarshal in module windows_core::imp`,
  `cannot find function marshaler in module windows_core::imp`,
  `cannot find function submit in crate windows_threading` →
  `could not compile windows-future (lib) due to 16 previous errors`, overall **exit 101**.
  (The failure is in `windows-future`, not in `windows-capture` code, so it is a hard
  dependency-level blocker on Linux.)
- `cargo check --target x86_64-pc-windows-gnu` → **exit 0** (`Finished dev profile in 13.00s`).
- docs.rs is Windows-targeted: `[package.metadata.docs.rs] default-target = "x86_64-pc-windows-msvc"`
  (`SRC/Cargo.toml.orig:69-74`).

**Consequence for `crates/video-capture`**: the dependency must be declared under
`[target.'cfg(windows)'.dependencies]`, and every use site inside the crate must be behind
`#[cfg(windows)]` (mirroring `crates/gamepad`), otherwise the Linux gate
(`./scripts/check.sh`) breaks — the crate cannot even be *compiled* on Linux.

---

## 2. Public API surface: start/stop, output production, muxing to playable MP4/MKV

Public modules (`SRC/src/lib.rs:171-194`): `capture`, `d3d11`, `dxgi_duplication_api`,
`encoder`, `frame`, `graphics_capture_api`, `graphics_capture_picker`, `monitor`,
`settings`, `window`; plus `pub use windows::Graphics::Capture::GraphicsCaptureItem` (`:164`)
and `pub(crate) mod winrt`.

### Capture start / stop

Trait-based handler (`SRC/src/capture.rs:279-498`):

```rust
pub trait GraphicsCaptureApiHandler: Sized {
    type Flags;
    type Error: Send + Sync;                                   // capture.rs:285
    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error>;            // :484
    fn on_frame_arrived(&mut self, frame: &mut Frame,
        capture_control: InternalCaptureControl) -> Result<(), Self::Error>;   // :487-491
    fn on_closed(&mut self) -> Result<(), Self::Error> { Ok(()) }             // :495-497 (default)
}
```

Two start styles, both on the trait:

```rust
fn start<T: TryInto<GraphicsCaptureItemType>>(settings: Settings<Self::Flags, T>)
    -> Result<(), GraphicsCaptureApiError<Self::Error>>                        // capture.rs:289
    where Self: Send + 'static, Self::Flags: Send;   // takes over the calling thread (Win32 message loop)

fn start_free_threaded<T: TryInto<GraphicsCaptureItemType> + Send + 'static>(
    settings: Settings<Self::Flags, T>)
    -> Result<CaptureControl<Self, Self::Error>, GraphicsCaptureApiError<Self::Error>>  // capture.rs:367
```

`CaptureControl` handle API (`capture.rs:95-171`): `is_finished()`, `into_thread_handle()`,
`halt_handle() -> Arc<AtomicBool>`, `callback() -> Arc<Mutex<T>>`, `wait()`, `stop()`.
In-loop stop: `InternalCaptureControl::stop(self)` (`graphics_capture_api.rs:82`). External stop:
set the `Arc<AtomicBool>` from `halt_handle()` (used by the CLI example with `ctrlc`,
`SRC/examples/cli.rs:103`).

Low-level alternative without the trait: `GraphicsCaptureApi::new::<T, E>(...)`
(`graphics_capture_api.rs:119`), `start_capture(&mut self) -> Result<(), Error>` (`:399`),
`stop_capture(mut self)` (`:414`), `halt_handle()` (`:425`).

Second capture backend (2.0.1, monitor-only, no WinRT item): `DxgiDuplicationApi::new(monitor)`,
`new_options(monitor, &[DxgiDuplicationFormat])`, `recreate()`, `recreate_options(...)`,
`acquire_next_frame(&mut self, timeout_ms: u32) -> Result<DxgiDuplicationFrame<'_>>`
(`SRC/src/dxgi_duplication_api.rs`); frame accessors `buffer()/buffer_crop()/buffer_with()/
buffer_crop_with()/save_as_image()` and `DxgiDuplicationFrameBuffer::as_nopadding_buffer()/
save_as_image()`. Empty updates are detected via `frame.frame_info().LastPresentTime == 0`.

Capture items: `Window::foreground()/from_name()/from_contains_name()/enumerate()/rect()/
title_bar_height()/monitor()/...` (`SRC/src/window.rs`); `Monitor::primary()/from_index()/
enumerate()/width()/height()/refresh_rate()/...` (`SRC/src/monitor.rs`); interactive picker
`GraphicsCapturePicker::pick_item() -> Result<Option<PickedGraphicsCaptureItem>>` with
`PickedGraphicsCaptureItem::size() -> windows::core::Result<(i32, i32)>`
(`SRC/src/graphics_capture_picker.rs`).

Frame access (`SRC/src/frame.rs`): `width()/height()` (`u32`), `dirty_regions()`,
`timestamp() -> Result<TimeSpan>`, `color_format()`, `as_raw_surface()/as_raw_texture()/
device()/device_context()/desc()`, `buffer(&mut) -> Result<FrameBuffer>`,
`buffer_crop(...)`, `buffer_without_title_bar()`, `save_as_image(path, ImageFormat)`;
`FrameBuffer::width/height/row_pitch/depth_pitch/color_format/has_padding/as_raw_buffer/
as_nopadding_buffer(&mut Vec<u8>)/save_as_image`.

Capture configuration (`SRC/src/settings.rs`): `Settings::new(item, CursorCaptureSettings,
DrawBorderSettings, SecondaryWindowSettings, MinimumUpdateIntervalSettings,
DirtyRegionSettings, ColorFormat, flags)` (`:134-154`), enums: `CursorCaptureSettings
{ Default, WithCursor, WithoutCursor }`, `DrawBorderSettings { Default, WithBorder,
WithoutBorder }`, `SecondaryWindowSettings { Default, Include, Exclude }`,
`MinimumUpdateIntervalSettings { Default, Custom(Duration) }`, `DirtyRegionSettings
{ Default, ReportOnly, ReportAndRender }`, `ColorFormat { Rgba16F = 10, Rgba8 = 28, Bgra8 = 87 }`
(default `Rgba8`).

### Output production

`VideoEncoder` (`SRC/src/encoder.rs:679`, constructors at `:862` and `:1013`):

```rust
pub fn new<P: AsRef<Path>>(video_settings: VideoSettingsBuilder,
    audio_settings: AudioSettingsBuilder, container_settings: ContainerSettingsBuilder,
    path: P) -> Result<Self, VideoEncoderError>                     // :862  → file output
pub fn new_from_stream(video_settings, audio_settings, container_settings, stream)
    -> Result<Self, VideoEncoderError>                              // :1013 → IRandomAccessStream
pub fn send_frame(&mut self, frame: &Frame) -> Result<(), VideoEncoderError>            // :1176
pub fn send_frame_with_audio(&mut self, frame: &mut Frame, audio_buffer: &[u8])        // :1210
pub fn send_frame_buffer(&mut self, buffer: &[u8], timestamp: i64)                     // :1258
pub fn send_audio_buffer(&mut self, buffer: &[u8], _timestamp: i64)                    // :1286
pub fn finish(mut self) -> Result<(), VideoEncoderError>                                // :1314
```

Model: `new()` builds a `MediaEncodingProfile` (`SetVideo`/`SetAudio`/`SetContainer`), an
uncompressed `Bgra8` `VideoEncodingProperties`, a `MediaStreamSource::CreateFromDescriptors`,
mpsc channels for video/audio, `SampleRequested` handlers on the MF thread pool with
`GetDeferral()`, then `PrepareMediaStreamSourceTranscodeAsync(...).join()` and a spawned thread
running `TranscodeAsync()`. `send_frame` pushes a `VideoEncoderSource::DirectX(IDirect3DSurface)`
sample; timestamps are relative to the first frame (`frame.timestamp().Duration - t0.Duration`,
`encoder.rs:1180-1189`). `finish()` sends `None` (end-of-stream) on both channels and joins the
transcode thread; `Drop` closes the channels and removes the handlers. Errors surfacing during
transcoding are reported through an `error_notify` atomic and joined/re-raised on the next send
(`encoder.rs:1200-1207`).

Still-image output: `ImageEncoder::new(ImageFormat, ImageEncoderPixelFormat)` +
`encode(&[u8], width, height) -> Vec<u8>` (`encoder.rs:123`, `:155`), formats
`ImageFormat { Jpeg, Png, Gif, Tiff, Bmp, JpegXr }` (also
`Frame::save_as_image` / `DxgiDuplicationFrame::save_as_image`).

### Muxing

**The crate muxes itself** — no external mp4/mkv crate needed. Encoding *and* container
multiplexing are done by Media Foundation's `MediaTranscoder`
(`encoder.rs:934-935` for the file path, `:1083-1084` for the stream path), container chosen by
`ContainerSettingsBuilder::default()` = **`ContainerSettingsSubType::MPEG4`** → `.mp4`
(`encoder.rs:418-431`, enum at `:602-613`: `ASF, MP3, MPEG4, AVI, MPEG2, WAVE, AACADTS, ADTS,
GP3 ("3GP"), AMR, FLAC`).

**MKV/WebM are NOT available** — they are not in the container list, and Media Foundation has
no Matroska sink. For the spec: mp4 = supported (`MPEG4`), mkv/webm = **not implemented**
(would need an extra muxer crate, e.g. `mp4`/`matroska`, which is a new crates.io dep →
user approval).

---

## 3. Encoder selection: NVENC / AMD AMF / CPU fallback

**There is no encoder-selection API in any published version.** The premise "NVENC / AMD
GPU encoder via dlopen of nvenc.dll / amf.dll" is **not** how this crate works.

- `rg -in 'nvenc|amf|openh264|x264|EncoderType|encoder_type|libloading|dll' $SRC/src $SRC/README.md $SRC/examples`
  → **0 matches**; the same grep over `windows-capture-1.0.12/src` → **0 matches**
  (and 1.0.12 did not depend on `windows` feature `Media_Transcoding` at all, i.e. it had no
  video encoder).
- Hardware acceleration is requested **unconditionally and unconfigurably**:
  `media_transcoder.SetHardwareAccelerationEnabled(true)?` — `SRC/src/encoder.rs:935` (file
  output) and `:1084` (stream output). There is no flag to disable it, no vendor selection, no
  query for which encoder Media Foundation picked.
- **Who actually picks the encoder**: Media Foundation. With hardware acceleration enabled, MF
  selects a vendor hardware MFT when one is installed (NVIDIA's NVENC-backed MF H.264/HEVC
  encoder, AMD's AMF-backed MF encoder); otherwise it uses Microsoft's software encoder MFTs
  ([H.264 Video Encoder](https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-encoder),
  [H.265 / HEVC Video Encoder](https://learn.microsoft.com/en-us/windows/win32/medfound/h-265---hevc-video-encoder)).
  Nothing is dlopened by `windows-capture`; the DLL discovery is MF's own COM/MFT enumeration.
- **Behaviour when no GPU encoder exists**: no crate-side fallback logic exists
  (`rg -in 'fallback' $SRC/src` → one unrelated DXGI comment,
  `SRC/src/dxgi_duplication_api.rs:244`). MF either resolves to the software MFT (typical for
  H.264, which ships with Windows) or fails to match the requested profile — the failure is
  surfaced verbatim as `VideoEncoderError::WindowsError` (HRESULT) from
  `VideoEncoder::new` / the transcode thread (`encoder.rs:185-220`, error re-raised on the next
  `send_*`, `encoder.rs:1200-1207`). Concretely: the crate's default codec is **HEVC**, and MF's
  H.265 encoder supports only Main Profile (`eAVEncH265VProfile_Main_420_8`) with YUV input
  subtypes per the MS docs — on a machine without a usable HEVC encoder, HEVC capture fails
  rather than degrading.
- **Implication for `crates/video-capture`**: pass
  `VideoSettingsBuilder::new(w, h).sub_type(VideoSettingsSubType::H264)` explicitly (H.264 + MP4
  is the universally playable/hardware-available combination), and treat encoder construction
  failure as a recoverable error (log + fall back to still-image capture) instead of assuming
  NVENC/AMF. There is no API to report "which encoder was used", so the spec should say
  "hardware encoding is used when Windows/Media Foundation provides a hardware encoder; the
  choice is not observable" and mark explicit NVENC/AMF/CPU selection **not implemented**.
- Encoding runs on the crate's own background transcode thread, so encode cost does not block
  `on_frame_arrived`; the callback only pushes samples into mpsc channels (`encoder.rs:1197`).

---

## 4. H.264 vs H.265/HEVC representation

`VideoSettingsSubType` (`SRC/src/encoder.rs:447-471`) is a single flat enum:

```
ARGB32, BGRA8, D16, H263, H264, H264ES, HEVC, HEVCES, IYUV, L8, L16, MJPG, NV12,
MPEG1, MPEG2, RGB24, RGB32, WMV3, WVC1, VP9, YUY2, YV12
```

- **H.264 = `H264`** (elementary-stream variant `H264ES`), mapped by `to_hstring()`
  (`encoder.rs:495-522`) to the WinRT media subtype string `"H264"` / `"H264ES"`.
- **H.265 = `HEVC`** — there is **no separate `H265` value**; H.265 and HEVC are the *same*
  enum variant (plus `HEVCES` elementary-stream variant), mapped to `"HEVC"` / `"HEVCES"`.
  So "h.264 vs h.265" is expressed as exactly two choices (`H264` vs `HEVC`), not a family.
- **Default is `HEVC`** (`VideoSettingsBuilder::new`, `encoder.rs:270-281`), i.e. out of the box
  the crate produces H.265 in MP4. The wrapper must set `H264` explicitly for broad playback.
- `VP9` exists as a subtype, but note VP9-in-MP4 support in MF is limited; not part of our scope.
- Audio side (`AudioSettingsSubType`, `encoder.rs:526-...`): `AAC, AC3, AACADTS, AACHDCP,
  AC3SPDIF, AC3HDCP, ADTS, ALAC, AMRNB, AWRWB, DTS, EAC3, FLAC, Float, MP3, MPEG, OPUS, PCM,
  WMA8, WMA9, Vorbis` — default `AAC` (`:340-369`).

---

## 5. Rate control (CBR / CQP / CRF)

Only one rate-control knob exists, and its exact name is `bitrate`:

```rust
VideoSettingsBuilder::new(width: u32, height: u32)   // encoder.rs:270, defaults below
    .sub_type(VideoSettingsSubType)                  // :283
    .bitrate(bitrate: u32)                           // :289  — bits per second
    .width(width: u32)                               // :295
    .height(height: u32)                             // :301
    .frame_rate(frame_rate: u32)                     // :307  — numerator, denominator fixed 1
    .pixel_aspect_ratio((u32, u32))                  // :313
    .disabled(bool)                                  // :320
```

Defaults of `VideoSettingsBuilder::new(w, h)`: `sub_type = HEVC`, **`bitrate = 15_000_000`**
(15 Mbps), `frame_rate = 60`, `pixel_aspect_ratio = (1, 1)`, `disabled = false`.

`bitrate` is applied as `properties.SetBitrate(self.bitrate)` in `VideoSettingsBuilder::build()`,
i.e. the MF average-bitrate attribute (`MF_MT_AVG_BITRATE`) → effectively a **CBR target
bitrate**. The CLI example exposes it verbatim as `--bitrate` (bits/sec, default
`15_000_000`) and `--frame-rate` (default 60) (`SRC/examples/cli.rs:177-182`).

**Not supported** (verified by absence): `rg -in 'cqp|crf|quantiz|rate_control|SetBitrateMode|quality_mode|abr|VBR|icodecapi|codecapi|keyframe|forcekeyframe|gop|MF_MT_MPEG2|MF_MT_VIDEO_PROFILE' $SRC/src`
→ **0 matches**. So there is no enum/parameter for rate-control mode, **no CQP**, **no CRF /
constant-quality / QP-based encoding**, no VBR/ABR, no quality knob, no GOP/keyframe interval,
no H.264/H.265 profile or level selection. Media Foundation *can* express CQP/quality modes via
`ICodecAPI` on its encoder MFTs, but `windows-capture` never opens `ICodecAPI`, so those modes are
unreachable through this crate's API. Marked in the spec as **not implemented** (spec item
"h.265 cqp/crf rate control" → unsupported by the library).

Audio rate control for completeness: `AudioSettingsBuilder::new()` defaults
`bitrate = 192_000`, `channel_count = 2`, `sample_rate = 48_000`, `bit_per_sample = 16`,
`sub_type = AAC`, `disabled = false` (`encoder.rs:350-369`); setters `bitrate`, `channel_count`,
`sample_rate`, `bit_per_sample`, `sub_type`, `disabled` (`:370-396`).

---

## 6. Resolution, frame rate, downscaling

- **Resolution**: set by `VideoSettingsBuilder::new(width, height)` / `.width()` / `.height()`
  (pixel sizes; e.g. `1280x720`, `1920x1080`). **Full desktop** = capture item
  `Monitor::primary()` or `Monitor::from_index(n)` (`SRC/src/monitor.rs`) → native monitor size
  (`Monitor::width()/height() -> u32`), or `Monitor::refresh_rate() -> u32` for the panel rate.
  Window capture: `Window::foreground()`, `Window::from_contains_name(...)`, `Window::rect()`,
  `Window::title_bar_height()`.
- **Frame rate**: `frame_rate(u32)` (30 or 60) — it only writes the container's frame-rate
  attribute; the delivered rate is whatever cadence the app pushes frames at. The upstream CLI
  example measures actual FPS from frame arrivals (`SRC/examples/cli.rs:88-96`), confirming the
  crate does not enforce a cadence. Optional OS-side throttle:
  `MinimumUpdateIntervalSettings::Custom(Duration)` (`settings.rs:69-94`), whose doc states it is
  a *minimum interval between eligible updates*, "not a target frame rate" and "does not guarantee
  a constant rate … consumers that require a fixed cadence must pace the output themselves";
  probe with `GraphicsCaptureApi::is_minimum_update_interval_supported()`.
  ⇒ 30/60 fps must be paced by our wrapper (drop/pace sends against `Frame::timestamp()`).
- **Downscaling / scaling: NOT supported.** `send_frame` uses the frame surface verbatim only
  when the sizes match exactly (`encoder.rs:1191-1195`); otherwise `build_padded_surface`
  creates a render target at the *target* size, clears it to black, and
  `CopySubresourceRegion` copies `copy_width = target_width.min(frame.width())`,
  `copy_height = target_height.min(frame.height())` from the **top-left**
  (`build_padded_surface`, `encoder.rs:1125-1160`, copy at `:1148-1160`;
  `create_cached_surface` at `:714-751`). Larger source ⇒ cropped; smaller source ⇒ black
  padding. There is no resize/scale path anywhere in the crate. So "encode 720p from a 1080p
  desktop" is **not** achievable via the library: encoder size must equal the captured item size,
  or we implement scaling ourselves (CPU resize into `send_frame_buffer`, or a D3D copy/shader —
  the latter would need `unsafe`/FFI and is out of scope for a `#![forbid(unsafe_code)]` crate).

---

## 7. Replay / ring buffer (last 0–600 s)

**Not implemented — the crate has no replay or ring-buffer API.**

- `rg -in 'replay|ring.?buffer|last.?seconds|pre.?buffer|keep.?frames' $SRC/src $SRC/README.md $SRC/examples`
  → **0 matches**; no type, method, or setting named for retaining past frames.
- The only buffering knobs are internal and tiny:
  `media_stream_source.SetBufferTime(Duration::from_millis(30).into())?`
  (`SRC/src/encoder.rs:906` and `:1055`) — Media Foundation's own sample buffer time, not a
  user-facing replay window; and `MinimumUpdateIntervalSettings::Custom(Duration)`
  (`settings.rs:69-94`), which is an update **throttle**, not history.
- Frames are delivered as borrowed `&mut Frame` inside `on_frame_arrived` and are recycled by
  Windows Graphics Capture after the callback returns, so history must be copied by the caller.

**How to implement the 0–600 s replay in `crates/video-capture`** (library-supported path, no
`unsafe`): in the callback, copy the frame out with `frame.buffer()?` /
`FrameBuffer::as_nopadding_buffer(&mut Vec<u8>)` (BGRA) plus `frame.timestamp()?` and push into
a bounded ring (`VecDeque<(i64 /*100 ns ticks*/, Vec<u8>)>` sized
`capacity = fps * seconds * width * height * 4`, dropping the oldest). On trigger, create a
`VideoEncoder` and replay with `send_frame_buffer(&buffer, timestamp)` — its `timestamp: i64`
is in 100 ns ticks (`TimeSpan::Duration` units) and the encoder normalizes it relative to the
first sample sent (`encoder.rs:1258-1273`). Memory cost: 1080p BGRA ≈ 8.3 MB/frame ⇒ 600 s at
30 fps ≈ 5 GB, so the wrapper must bound the ring by *bytes* and cap the window (spec should
note this ceiling rather than promising 600 s at 1080p/60 fps). Encoding a 600 s replay is also
CPU-bound (MF transcode of already-captured frames, no live capture pacing), so the encoder must
be fed on a dedicated thread while capture keeps running.

---

## 8. Audio capture: desktop/system audio and microphone

**The crate captures no audio at all — audio is ingest-only.**

- `rg -in 'audioclient|IAudioCaptureClient|wasapi|microphone|mic\b|enumerate.?audio|audiorender' $SRC/src $SRC/examples`
  → **0 matches**; no WASAPI, no device enumeration, no loopback, no microphone.
- Audio enters only through `VideoEncoder`: `AudioEncoderSource::Buffer(Vec<u8>)`
  (`encoder.rs:236-241`) = interleaved PCM bytes; the stream descriptor is created with
  `AudioEncodingProperties::CreatePcm(sample_rate, channel_count, bits_per_sample)`
  (`encoder.rs:889-896`) and MF transcodes it to the *output* codec
  (`AudioSettingsBuilder::sub_type`, default **AAC @ 192 kbps / 2 ch / 48 kHz / 16 bit**,
  `encoder.rs:350-369`). Block alignment is computed as `channels * bits/8`
  (`encoder.rs:901`).
- API: `send_audio_buffer(&mut self, buffer: &[u8], _timestamp: i64)` (`encoder.rs:1286`) — the
  timestamp argument is **ignored by design**: "NOTE: The provided `timestamp` is ignored; we use
  a monotonic audio clock" (`:1285`, `:1295-1298`: ticks = `samples_sent * 10_000_000 / sample_rate`),
  and `send_frame_with_audio(&mut self, frame: &mut Frame, audio_buffer: &[u8])` (`:1210`) does the
  same. Consequence: the audio stream starts at t=0 and advances purely by delivered sample
  count, so feeding audio slower/faster than real time drifts A/V sync; the wrapper must feed
  audio at the real capture cadence (or disable audio: `AudioSettingsBuilder::default().disabled(true)`).
- **Desktop/system audio (loopback) and microphone are therefore "not implemented" by the
  library.** Options for the wrapper: (a) a hand-written WASAPI loopback shim → requires `unsafe`
  FFI, which the project forbids (`#![forbid(unsafe_code)]`), so it is not viable in a strict
  `forbid` crate; (b) a new crates.io dep such as `cpal` (safe API, Windows backend) → **needs
  explicit user approval** before adding. Neither is inherited from `windows-capture`.

---

## 9. Dependencies (crates.io) — direct, transitive, and what is NOT used

Direct dependencies of `windows-capture 2.0.1`
(source: <https://crates.io/api/v1/crates/windows-capture/2.0.1/dependencies> and
`SRC/Cargo.toml.orig:21-63`) — all normal (non-dev), **no optional, no features of our own**:

| crate | version | notes |
| --- | --- | --- |
| `windows` | `^0.62.2` | 27 features, listed verbatim below |
| `windows-future` | `^0.3.2` | WinRT async await support |
| `parking_lot` | `^0.12.5` | mutexes for the callback handle |
| `rayon` | `^1.12.0` | parallel pixel work / MF thread pool helpers |
| `thiserror` | `^2.0.18` | error enums |

`windows` features (verbatim, `SRC/Cargo.toml.orig:23-53`):
`Foundation_Metadata, Foundation, Graphics_Capture, Graphics_DirectX_Direct3D11,
Graphics_Imaging, Media_Core, Media_MediaProperties, Media_Transcoding,
Security_Cryptography, Storage_Streams, Storage, System, Win32_Devices_Display,
Win32_Foundation, Win32_Graphics_Direct3D, Win32_Graphics_Direct3D11, Win32_Graphics_Dwm,
Win32_Graphics_Dxgi_Common, Win32_Graphics_Gdi, Win32_System_Com,
Win32_System_LibraryLoader, Win32_System_ProcessStatus, Win32_System_Threading,
Win32_System_WinRT_Direct3D11, Win32_System_WinRT_Graphics_Capture, Win32_UI_HiDpi,
Win32_UI_Shell, Win32_UI_WindowsAndMessaging, System_Threading`.

Dev-only (examples, **not inherited** by a consumer): `clap ^4.6.1` (feature `derive`),
`ctrlc ^3.5.2` (`SRC/Cargo.toml.orig:65-67`).

What is **absent** (answers the muxing/audio/encoder-backend questions):

- **No muxing / mp4 / mkv crate** — multiplexing is Media Foundation `MediaTranscoder`
  (`encoder.rs:934-935`, `:1083-1084`); no `mp4`, `matroska`, `webm`, `ffmpeg`, `muxer` dep.
- **No audio-capture crate** — no `cpal`, `wasapi`, `minimal-alsa`, `rodio`, `sound-source`.
- **No software encoder** — no `openh264`, `x264`, `x265`, `libx264`.
- **No hardware-encoder bindings** — no `nvenc`, `amf`, `libloading`, no `.dll`/`.so` blobs in
  the published package; encoding is MF-only.

Full transitive closure actually locked for a consumer crate (from `./.tmp/wc-check/Cargo.lock`,
35 packages): `bitflags 2.13.2, cfg-if 1.0.5, crossbeam-deque 0.8.8, crossbeam-epoch 0.9.21,
crossbeam-utils 0.8.23, either 1.19.0, libc 0.2.191, lock_api 0.4.14, parking_lot 0.12.5,
parking_lot_core 0.9.12, proc-macro2 1.0.107, quote 1.0.47, rayon 1.12.0, rayon-core 1.13.0,
redox_syscall 0.5.18, scopeguard 1.2.0, smallvec 1.16.5, syn 2.0.119, syn 3.0.9,
thiserror 2.0.21, thiserror-impl 2.0.21, unicode-ident 1.0.27, windows 0.62.2,
windows-capture 2.0.1, windows-collections 0.3.2, windows-core 0.62.2, windows-future 0.3.2,
windows-implement 0.60.2, windows-interface 0.59.3, windows-link 0.2.1, windows-numerics 0.3.1,
windows-result 0.4.1, windows-strings 0.5.1, windows-threading 0.2.1`. There is **no
`windows-targets` / `windows_x86_64_*` import-lib shim** in that lock, because `windows 0.62`
binds DLLs with `raw-dylib` (see §1) — exactly what makes the MinGW-w64 cross-build work.

Overlap with the project's existing lock (`Cargo.lock`): `windows 0.62.2`, `windows-core 0.62.2`,
`windows-future 0.3.2`, `windows-implement 0.60.2`, `windows-interface 0.59.3`,
`windows-collections 0.3.2`, `windows-link/result/strings/threading/numerics`, `parking_lot`,
`thiserror`, `crossbeam-*`, `either`, `libc`, `cfg-if`, `smallvec`, `scopeguard`, `lock_api`,
`proc-macro2/quote/syn/unicode-ident`, `bitflags` are **already locked** (the `windows` chain is
currently pulled in transitively by `xcap 0.9.8`). Diff of the two name sets
(`comm -23 ./.tmp/wc-deps.txt ./.tmp/proj-deps.txt`) → only **3 new packages**:

1. `windows-capture 2.0.1`
2. `rayon 1.12.0`
3. `rayon-core 1.13.0`

Caveat for approval: adding `windows-capture` **widens the feature set** of the already-locked
`windows 0.62.2` (cargo unifies features workspace-wide, so `xcap`'s Windows build also compiles
those 27 modules) — that is a compile-time/binary-size change, not a new package. The published
manifest carries no `[workspace]` table, so it cannot disturb our workspace membership.

---

## 10. MSRV / edition and caller-side `unsafe` requirements

**Toolchain**:

- `edition = "2024"` (`SRC/Cargo.toml.orig:5`); **no `rust-version`** in the manifest
  (crates.io reports `rust_version: null`) → **no declared MSRV**; effectively ≥ Rust 1.85
  (edition 2024) and Cargo ≥ 1.87 (`resolver = "3"`, `Cargo.toml.orig:19`). Project pins Rust
  1.99.1 stable ⇒ compatible.
- The published package ships `rust-toolchain.toml` (`[toolchain] channel = "stable"`,
  `targets = ["x86_64-pc-windows-msvc"]`) — it only affects upstream development; a dependency's
  `rust-toolchain.toml` is not applied to our workspace.
- Transitive `windows 0.62.2` and `windows-future 0.3.2` are `edition = "2021"`, no `rust-version`.
- No cargo features on `windows-capture`; nothing to configure beyond target selection.

**`unsafe`**: the crate never asks its caller to write `unsafe`.

- `rg 'pub unsafe fn|unsafe fn' $SRC/src` → **0 matches**: no `unsafe fn` in the API at all.
- Crate root attributes: `#![warn(clippy::nursery)]`, `#![warn(clippy::cargo)]`,
  `#![warn(clippy::multiple_crate_versions)]`, `#![warn(missing_docs)]`
  (`SRC/src/lib.rs:158-161`) — **no `#![forbid(unsafe_code)]`**, but that is upstream's choice and
  does not propagate to our crate.
- Internal `unsafe` (all confined inside the crate, invisible to consumers):
  `SRC/src/d3d11.rs:48` (`unsafe impl<T> Send for SendDirectX<T>`), `:98/:110/:117/:135/:145/:179/:212/:242/:282`
  (D3D11 device/surface creation, `CreateDirect3D11SurfaceFromDXGISurface`);
  `SRC/src/window.rs:78` (`unsafe impl Send for Window`), `:88` (`GetForegroundWindow`), `:106` (`FindWindowW`);
  `SRC/src/encoder.rs:219-220` (`unsafe impl Send/Sync for VideoEncoderError`), `:1370`
  (`unsafe impl Send for VideoEncoder`, with `#[allow(clippy::non_send_fields_in_send_ty)]`);
  `SRC/src/capture.rs` message loop (`GetMessageW`/`TranslateMessage`/`DispatchMessageW`).
- ⇒ A `crates/video-capture` with `#![forbid(unsafe_code)]` can consume `windows-capture` without
  any local `unsafe` (per AGENTS.md, third-party internal unsafe is outside the rule).
- Raw-pointer helpers are **safe** functions (no `unsafe` needed to call them):
  `Window::from_raw_hwnd(hwnd: *mut c_void)`, `Monitor::from_raw_hmonitor`,
  `Frame::as_raw_surface()/as_raw_texture()` — we simply must not fabricate pointers.
- Thread/`Send` constraints to respect in our wrapper: the handler must be `Send + 'static`
  (`capture.rs:293`), `Flags: Send`; `GraphicsCaptureApi::new::<T, E>` requires `T: Send + 'static`,
  `E: Send + Sync`; `VideoEncoder` is `Send` but **not `Sync`** ⇒ one encoder per capture thread,
  no sharing; `start()` takes over the calling thread and initializes WinRT itself
  (`WinRT::new()`, `capture.rs:297`) ⇒ run capture on a dedicated spawned thread (matches the
  project's "capture runs on a spawned thread" rule) or use `start_free_threaded` +
  `halt_handle()`/`wait()`/`stop()` for graceful shutdown.
- Linux caveat: `cargo doc -p windows-capture` / `cargo check` on the host cannot run (§1), so
  crate API knowledge here comes from the unpacked source, not rendered local docs.

---

## 11. Complete capture-to-file example (README / crate docs)

The upstream crate doc (`SRC/src/lib.rs:35+`, marked `````no_run`````) and README §Usage
(<https://github.com/NiiightmareXD/windows-capture#usage>, README lines 51-176) ship the same
example as `SRC/examples/graphics_capture.rs` (verbatim, 123 lines):

```rust
use std::io::{self, Write};
use std::time::Instant;

use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::graphics_capture_picker::GraphicsCapturePicker;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};

struct Capture {
    encoder: Option<VideoEncoder>,
    start: Instant,
}

impl GraphicsCaptureApiHandler for Capture {
    type Flags = (i32, i32);
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let encoder = VideoEncoder::new(
            VideoSettingsBuilder::new(ctx.flags.0 as u32, ctx.flags.1 as u32),
            AudioSettingsBuilder::default().disabled(true),
            ContainerSettingsBuilder::default(),
            "video.mp4",
        )?;
        Ok(Self { encoder: Some(encoder), start: Instant::now() })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, capture_control: InternalCaptureControl) -> Result<(), Self::Error> {
        print!("\rRecording for: {} seconds", self.start.elapsed().as_secs());
        io::stdout().flush()?;
        self.encoder.as_mut().unwrap().send_frame(frame)?;
        // Other uses: frame.save_as_image("frame.png", ImageFormat::Png)?;
        //             let data = frame.buffer()?;  // raw data, full control
        if self.start.elapsed().as_secs() >= 6 {
            self.encoder.take().unwrap().finish()?;   // muxes and closes video.mp4
            capture_control.stop();
            println!();
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        println!("Capture session ended");
        Ok(())
    }
}

fn main() {
    let item = GraphicsCapturePicker::pick_item().expect("Failed to pick item");
    let Some(item) = item else { println!("No item selected"); return };
    let size = item.size().expect("Failed to get item size");

    let settings = Settings::new(
        item,
        CursorCaptureSettings::Default,
        DrawBorderSettings::Default,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        size,
    );

    Capture::start(settings).expect("Screen capture failed");   // takes over this thread
}
```

Notes on that flow: audio disabled ⇒ pure video MP4; container default `MPEG4`; codec default
`HEVC`; `finish()` is what writes/muxes and must be called before `capture_control.stop()`.

Monitor capture (no interactive picker — the shape our gamepad-trigger flow needs), from
`SRC/examples/cli.rs:56-113`: flags carry `width/height/bitrate/frame_rate/path`, then

```rust
let video_settings = VideoSettingsBuilder::new(ctx.flags.width, ctx.flags.height)
    .bitrate(ctx.flags.bitrate)
    .frame_rate(ctx.flags.frame_rate);
let encoder = VideoEncoder::new(video_settings,
    AudioSettingsBuilder::default().disabled(true),
    ContainerSettingsBuilder::default(), &ctx.flags.path)?;
// in on_frame_arrived: send_frame(frame), and on the external stop flag:
//   self.encoder.take().unwrap().finish()?; capture_control.stop();
```

with the item selected as `Monitor::from_index(n)` / `Window::from_contains_name(...)` and the
stop driven by `Ctrl+C` through an `Arc<AtomicBool>` (`examples/cli.rs:21`, `:103`).

Two more upstream examples worth knowing: **stream encoding** (`README` §Stream-based encoding
example, lines 177-294) uses `VideoEncoder::new_from_stream` with
`windows::Storage::Streams::InMemoryRandomAccessStream` — that path requires the `windows` crate
types in *our* code, i.e. `windows` would become a direct dependency of `crates/video-capture`
(needs approval); and **DXGI Desktop Duplication** (`README` lines 295-321):
`Monitor::primary()` → `DxgiDuplicationApi::new(monitor)` → `acquire_next_frame(33)` →
`save_as_image("dxgi_screenshot.png", ImageFormat::Png)`.
Docs: <https://docs.rs/windows-capture/latest/windows_capture/> (module list),
<https://docs.rs/windows-capture/latest/windows_capture/encoder/index.html> (encoder API).

---

## Not supported / caveats (for the spec: mark these "not implemented")

| Item | Status | Evidence |
| --- | --- | --- |
| MKV / WebM container | **not supported** | container list `ASF, MP3, MPEG4, AVI, MPEG2, WAVE, AACADTS, ADTS, GP3, AMR, FLAC` (`encoder.rs:602-613`); MP4 = `MPEG4` (default) |
| Explicit encoder choice (NVENC / AMD AMF / CPU) | **not exposed** | only `MediaTranscoder::SetHardwareAccelerationEnabled(true)` hard-coded (`encoder.rs:935`, `:1084`); no `EncoderType`, no `nvenc`/`amf`/`libloading` anywhere |
| Which encoder was used (observable) | **not queryable** | no API; MF decides |
| Guaranteed software fallback | **not guaranteed / not handled** | no fallback logic (`rg fallback` → unrelated comment); failure surfaces as `VideoEncoderError::WindowsError` |
| CQP / CRF / constant-quality modes | **not supported** | only `bitrate(u32)` (`encoder.rs:289`) → `SetBitrate` (CBR target); no `ICodecAPI`, no rate-control-mode enum |
| VBR/ABR, quality mode, GOP/keyframe interval, H.264/H.265 profile+level | **not supported** | builder method list is complete: `new/sub_type/bitrate/width/height/frame_rate/pixel_aspect_ratio/disabled` (`encoder.rs:270-338`) |
| Separate `H265` value | **does not exist** | H.265 == `HEVC` (`encoder.rs:447-471`, `to_hstring` `:495-522`); default subtype is HEVC ⇒ wrapper must set `H264` |
| Downscaling / rescaling to 720p/1080p | **not supported** | `build_padded_surface` crops (top-left) or black-pads to target size (`encoder.rs:1125-1160`); no scale path |
| Constant frame rate guarantee | **not provided** | `frame_rate` is a container attribute only; `MinimumUpdateIntervalSettings::Custom` "does not guarantee a constant rate" (`settings.rs:69-94`) ⇒ we pace |
| Replay / ring buffer (0–600 s) | **not implemented by the library** | no replay/ring API; only `SetBufferTime(30 ms)` (`encoder.rs:906`) ⇒ wrapper-side ring of copied BGRA frames + `send_frame_buffer` |
| Desktop/system audio capture (loopback) | **not implemented** | no WASAPI/`IAudioCaptureClient` in the crate (grep → 0) |
| Microphone capture | **not implemented** | same; audio is ingest-only (`AudioEncoderSource::Buffer`, `encoder.rs:236`) |
| Audio timestamp fidelity | **caveat** | `send_audio_buffer(buffer, _timestamp)` ignores the timestamp and uses a monotonic sample-count clock (`encoder.rs:1285-1298`) ⇒ feed at real-time cadence or A/V drifts |
| Non-Windows build | **fails** | no `cfg(windows)` gating; Linux `cargo check` exit 101 in `windows-future 0.3.2` ⇒ dependency + code must be `cfg(windows)`-gated |
| Caller-side `unsafe` | **none required** | 0 `unsafe fn` in the crate; internal unsafe only ⇒ `#![forbid(unsafe_code)]` compatible |
| Interactive picker | **caveat** | `GraphicsCapturePicker::pick_item()` opens a system dialog ⇒ use `Monitor`/`Window` items for the hotkey/gamepad-trigger flow |
| Stream output | **caveat** | `new_from_stream` (`encoder.rs:1013`) needs `windows::Storage::Streams` types in our code ⇒ `windows` becomes a direct dep (approval) |
| Error timing | **caveat** | transcode failures are observed on the *next* `send_*`/`finish()` via `error_notify` (`encoder.rs:1200-1207`) |
| Dependency approval burden | **3 new packages** | `windows-capture 2.0.1`, `rayon 1.12.0`, `rayon-core 1.13.0`; `windows 0.62.2` already locked via `xcap 0.9.8` but its feature set widens by 27 features (§9) |
