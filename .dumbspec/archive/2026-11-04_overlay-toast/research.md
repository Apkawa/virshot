# Research — overlay/toast for virshot (facts only, gathered 2026-11-04)

## Workspace / manifests
- Root `Cargo.toml`: package `virshot` v0.1.0, **edition 2024**, **no `[workspace]`**; deps:
  `xcap 0.9.8 (features wgc)`, `image 0.25 (default-features=false, png/jpeg/webp)`, `chrono 0.4.45`,
  `thiserror 2`, `eframe 0.37.0 (default-features=false, features glow/default_fonts/x11/wayland)`,
  `serde 1.0.229`, `toml 1.1.8`, `tracing 0.1.44`, `tracing-subscriber 0.3.23 (env-filter)`,
  `clap 4.6.7`, `tray-icon 0.26.1 (ksni)`, `egui 0.37.0`, `global-hotkey 0.9.0`, `rfd 0.17.2`.
  Dev-dep `rusty-hook ^0.11.2`. `[profile.release] opt-level=z, lto=thin, strip=true`.
- `gamepad = { path = "crates/gamepad" }` only under `[target."cfg(windows)".dependencies]`
  (GameInput module is `#[cfg(windows)]`; Linux build never compiles it).
- `crates/gamepad`: name `gamepad`, v0.1.0, edition 2024, **zero deps**, explicit `[lib]`;
  deliberately NOT a workspace member (own Cargo.lock + target/); tested via
  `cd crates/gamepad && ../../cargo.sh test`. Has `examples/buttons.rs` (console tester, Windows).
- `hidpad-poc/Cargo.toml` exists standalone (historical PoC), must stay excluded from any workspace.
- Toolchain pinned via `rust-toolchain.toml` (`channel = "stable"`); no `rust-version` in manifests.

## Locked stack relevant to overlay
- `Cargo.lock` contains: `eframe 0.37.0`, `egui 0.37.0`, `egui-winit 0.37.0`, `egui_glow 0.37.0`,
  `egui-wgpu 0.37.0` (not compiled), `winit 0.30.13`, `glow 0.17.0`, `glutin 0.32.3`,
  `glutin-winit 0.5.0`. All via eframe's glow backend — using them from new crates adds
  **no new crates.io versions**, only new direct dependency declarations.
- `.tmp/overlay.md` skeleton uses older versions (egui 0.31, glow 0.16) — align to the lock instead.
- Vendored sources present under `.cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

## Content-protection / click-through APIs (no unsafe needed)
- winit 0.30.13 (`src/window.rs`): `WindowAttributes.content_protected` (:120),
  `with_content_protected` (:426), `set_content_protected` (:1397); Windows impl implements it.
  Click-through: `set_ignore_cursor_events` is **gone**; API is
  `Window::set_cursor_hittest(bool)` (:1575) with impls for windows (:632), x11 (:1623),
  wayland (:585).
- egui 0.37 (`src/viewport.rs`): `ViewportCommand::ContentProtected(bool)` (:1190) — **command only**,
  `ViewportBuilder` has no content_protected field. `ViewportBuilder::with_mouse_passthrough(bool)`
  (:678) → `ViewportCommand::MousePassthrough` (:1200); `RequestPaintWhileHidden` (:1239).
- egui-winit 0.37 (`src/lib.rs`): maps `ContentProtected(v) => window.set_content_protected(v)` (:2134),
  `MousePassthrough(p)` (:2153), builder path `set_cursor_hittest(!mouse_passthrough)` (:2492).
- Linux content-protection support in winit 0.30.13: not verified from source — treat as likely
  no-op/stub on X11/Wayland; screenshot-exclusion must be verified/expected primarily on Windows.

## Existing toast implementation in virshot (to extract/move)
- `src/overlay.rs` (182 lines): notification rules + state machine —
  `Notification {Success{path}, Error{message}}` (:37), `ToastMailbox = Arc<Mutex<Option<Notification>>>`
  (:43), `push/take_latest/should_show/hold_ms/toast_text`, `FADE_MS=250` (:28), `ERROR_HOLD_MS=3000`
  (:32), `ToastPhase {Hold, FadeOut, Gone}`, `ToastState` (new/replace/phase/alpha/is_gone, :123-177).
- `src/gui.rs` (1705 lines) owns window creation/placement/painting:
  `gui::run` (:1662) builds `eframe::NativeOptions`, root viewport `.with_transparent(true)` (:1673),
  `eframe::run_native` (:1682); toast child viewport per monitor via
  `ctx.show_viewport_deferred(id, builder, cb)` (:1268); builder (:1253-1262):
  `with_title("virshot").with_inner_size(size).with_visible(false).with_active(false)
  .with_decorations(false).with_transparent(true).with_mouse_passthrough(true).with_always_on_top()`
  + optional `.with_position(...)`; close/replacement via `ViewportCommand::Close` (:1290-1300);
  placement helpers `toast_window_size` (:241), `toast_position` (:249), `toast_positions` (:289),
  `toast_placements` (:373); monitor geometry via `ViewportInfo::monitor_size` (:401);
  visible-after-first-paint sequencing (`viewport_has_window` :213,
  `RequestPaintWhileHidden`+`Visible(true)` via `toast_show_commands` :531); fade alpha re-sent
  each pass; time injected via `egui::Context::input(|i| i.time)` in `update_toast` (:1210-1220),
  mailbox drained in `logic` pass (:1602). Fixed insets: 30px vertical / 60px horizontal,
  bottom corners ~48px above taskbar (gui.rs:182-183; `windows-sys` rcWork rejected — unsafe).
- `docs/spec.md` §4.3 "Оверлей-уведомление (тост)" (~186-220): one toast replaces current; one
  window per monitor; transparent/undecorated/always-on-top/mouse-passthrough; Wayland ignores
  position/always-on-top (passthrough works); X11 transparency only at window creation; fade ~250ms;
  hold `duration_ms` default 2000; errors ~3000ms; shown after shot; errors show even when
  `overlay.enabled=false`. Config block `[overlay]` (:341-346): `enabled`, `position` (4 corners),
  `duration_ms`, `on_all_monitors`. **`content_protected` is absent from docs/spec.md** — screenshot
  exclusion is a NEW requirement.
- `.dumbspec/backlog.md` line 9 (under `## toast`): extract overlay-drawing into `crate/toast`
  + minimal test app — exactly this task's direction.

## Capture / event-emission point
- `src/capture.rs`: `CaptureCommand {mode, settings, toast: ToastMailbox}` (:116-124);
  `spawn_capture` (:149) = detached `std::thread::spawn`; `run_capture` (:164-172) runs `capture_core`
  then `push(&toast, toast_notification(&outcome))` (:166) — **single success/fail emission point**
  (`toast_notification` :131). Capture: `xcap::Monitor::capture_image()` (:193),
  `xcap::Window::capture_image()` (:208), `foreground_window()` (:259).
- `src/main.rs`: `overlay::new_mailbox()` once per process (:250 headless, :343 resident), passed to
  `gui::run` (:396); `gamepad::spawn_pump` (:375); GUI-failure idle loop (:416-448) also spawns
  captures with the same mailbox. Modules declared :62-72; `#![forbid(unsafe_code)]` at :59;
  `#![windows_subsystem = "windows"]` at :60.
- Threads: capture detached threads; gamepad pump own thread (`spawn_pump`, gamepad.rs:363-390,
  Linux stub :398); logging writer thread + mpsc (logging.rs:88-116); hotkey manager on MAIN thread
  (WM_HOTKEY/X11 affinity, hotkeys.rs:23-40). Cross-thread state = latest-only
  `Arc<Mutex<Option<T>>>` mailboxes.

## Linux capture / platform notes
- xcap 0.9.8 Linux backends: `xorg_capture.rs` (X11/xcb) + `wayland_capture.rs` (runtime detect);
  Linux deps `libwayshot-xcap 0.3.3`, `pipewire`, `xcb`; `wgc` feature is windows-only.
- src/capture.rs:253-258: Linux/X11 foreground via `_NET_ACTIVE_WINDOW` (XWayland only);
  full-screen falls back to primary monitor.
- Linux dev packages (CONTRIBUTING.md): libegl1-mesa-dev, libgbm-dev, libudev-dev, libwayland-dev,
  libpipewire-0.3-dev, libclang-dev, pkg-config.

## Gates / build
- `scripts/check.sh` = `./cargo.sh check` + `./cargo.sh test` + `./cargo.sh clippy -- -D warnings`
  (Linux; no fmt). `scripts/check_full.sh` = check.sh + `cargo.sh fmt` + `fmt --check`.
  `cargo.sh` sets `CARGO_HOME=./.cache/cargo`.
- Windows-gnu: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` (compile-only, not in
  check.sh); release build → `target/x86_64-pc-windows-gnu/release/virshot.exe` (~0.84 MB,
  <10 MB exe budget drives eframe feature trimming).
- Pre-commit via rusty-hook: `./scripts/check.sh && ./cargo.sh fmt && git add -u . && ./cargo.sh fmt --check`.
- `scripts/cratedoc.sh` renders locked crate docs; `scripts/ui-verify.sh` exists for UI verification.

## Unsafe policy precedent
- virshot: crate-root `#![forbid(unsafe_code)]` (main.rs:59). gamepad: `#![deny(unsafe_code)]`
  (lib.rs:19) + module-scoped `#![allow(unsafe_code)]` only in gameinput.rs:25 (COM shim).
- New crates must follow `#![forbid(unsafe_code)]` at crate root (overlay content-protection and
  click-through are reachable via safe egui/winit APIs — no FFI needed).

## Conventions
- One module = one file under `src/`; tests inline `mod tests` or sibling `<module>.tests.rs`.
- gamepad-style `examples/` precedent: console tester (`crates/gamepad/examples/buttons.rs`).
- Errors: thiserror typed enums; logging: tracing in virshot, gamepad keeps stderr notes (zero deps).

## Wake / repaint contract (verified in src/gui.rs)
- eframe logic pass schedules `ctx.request_repaint_after(TRAY_POLL_TICK)` (gui.rs:1607) — the loop
  wakes periodically even idle, so mailbox-driven state (toast) drains promptly.
- Toast state changes trigger `ctx.request_repaint_of(viewport.id)` (gui.rs:1288) and
  `ctx.request_repaint()` (gui.rs:1292); root-level scheduling at :1568/:1580.
- `egui::Context::request_repaint()` is safe to call from other threads (Context is Send+Sync) —
  a capture thread could wake the loop directly instead of relying on the poll tick.
- Consequence for synchronous overlay/toast libs: timers only advance when the host repaints;
  libs must compute state from injected `now` and never own timers/threads themselves.

## Stage 6 research — Windows GL bootstrap (A)

Symptom (Windows host run of `target/x86_64-pc-windows-gnu/release/examples/overlay_demo.exe`):
overlay window flashes, then three identical lines
`overlay: GL context creation attempt failed: [ffffffffc007000d] OS Error -1073283059 (FormatMessageW() returned error 317)`,
then `demo: spawn_window failed: overlay GL bootstrap failed: could not create a GL context for the overlay: ...`.
Raw `GetLastError()` = 0xC007000D; `FormatMessageW` error 317 = `ERROR_INVALID_MSGTABLE_OFFSET` (no message text for the code).

### Root cause: the context attributes omit the window handle, so `SetPixelFormat` is never applied to the overlay window's HDC

Evidence chain (vendored sources, line numbers cited):

1. **WGL-first display selection succeeds; the failure is at context creation, not config selection.**
   `glutin-winit-0.5.0/src/lib.rs:107-111` creates the winit window **first** on the WGL backend
   (`// XXX with WGL backend window should be created first.`), passes its raw handle to
   `create_display` (`:115-121`) and into the config template
   (`template_builder.compatible_with_native_window(...)`, `:124-127`). `ApiPreference::FallbackEgl`
   maps to `DisplayApiPreference::WglThenEgl(handle)` on Windows
   (`glutin-winit-0.5.0/src/lib.rs:178-182`); `glutin-0.32.3/src/display.rs:242-248` tries
   `WglDisplay::new` first and only falls back to EGL if it fails — it did not fail (opengl32.dll
   loaded, `glutin-0.32.3/src/api/wgl/display.rs:59-66`), so the display is WGL. The user-visible
   error came from `create_context` (`crates/overlay/src/gl_boot.rs:214-231`), i.e.
   `DisplayBuilder::build` (`glutin-winit-0.5.0/src/lib.rs:97-147`) returned `Ok`: window creation
   and `wglChoosePixelFormatARB` config enumeration both succeeded
   (`glutin-0.32.3/src/api/wgl/config.rs:31-48,155-274`). So the requested config attributes
   (alpha 8, transparency, `TRANSPARENT_ARB` at `config.rs:246`) are NOT the failing part.

2. **In the WGL path no `SetPixelFormat` call ever happens on the real window's DC before context
   creation.**
   - `glutin-0.32.3/src/api/wgl/config.rs:35-39`: `find_configs` stores `hdc = GetDC(window hwnd)` in
     the config (`ConfigInner.hdc`, `:476-478`); the ARB path stores no
     `PIXELFORMATDESCRIPTOR` and never calls `SetPixelFormat` — only
     `wglChoosePixelFormatARB` (`:257`).
   - `glutin-0.32.3/src/api/wgl/mod.rs:48-141` (`load_extra_functions`): extension loading runs on a
     **dummy window** ("WglDummy Window", `:77-110`) whose own DC gets a dummy pixel format
     (`:116-121`); the dummy is destroyed (`:136-139`) — the real overlay window's DC is never
     pixel-format-set.
   - On the WGL backend `glutin_winit::finalize_window` (which would call
     `config.apply_on_native_window` → `SetPixelFormat`, `glutin-0.32.3/src/api/wgl/config.rs:295-310`)
     is **not invoked** by `DisplayBuilder::build` (`glutin-winit-0.5.0/src/lib.rs:140-144` runs it
     only for non-WGL platforms).

3. **Our bootstrap passes `None` as the raw window handle, which is exactly the documented WGL
   requirement violation.**
   - `crates/overlay/src/gl_boot.rs:221`: `builder.build(None)` (comment at `:212-213` claims WGL
     does not need the handle — it does).
   - `glutin-0.32.3/src/context.rs:232-236` (docs): *"WGL: you **must** pass a `raw_window_handle` if
     you plan to use this context with that window."*
   - `glutin-0.32.3/src/api/wgl/context.rs:35-41`: with `raw_window_handle = Some(Win32)` glutin calls
     `config.apply_on_native_window(handle)` (= `SetPixelFormat` on a fresh `GetDC` of the window,
     descriptor NULL — allowed for ARB-chosen formats) and then uses that window DC; with `None` it
     falls back to `config.inner.hdc`, the `GetDC(hwnd)` handle from `find_configs` whose pixel
     format was never set.
   - Microsoft docs (`wglCreateContext`, wingdi.h): *"Set the pixel format of the device context
     before creating a rendering context"* — https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-wglcreatecontext
     A creation call against a pixel-format-less DC fails and `GetLastError()` reports the opengl32
     failure code.

4. **Why all three attempts fail identically ⇒ DC-level precondition failure, not profile/version
   incompatibility.** `create_context` tries default (Core 3.3 via `pick_profile(None,None)`,
   `glutin-0.32.3/src/context.rs:672-684`), then GLES2, then Compatibility 2.1
   (`crates/overlay/src/gl_boot.rs:216-220`). Attempts 1 and 3 go through
   `wglCreateContextAttribsARB` (`glutin-0.32.3/src/api/wgl/context.rs:49-50,79-119`); a driver that
   rejects a profile yields different errors, while all attempts share the same HDC and fail with the
   same last-error value — consistent with the shared "pixel format not set on the DC" precondition.
   (If `WGL_EXT_create_context_es2_profile` were absent, attempt 2 would print
   "extension to create ES context with wgl is not present" — `context.rs:102-104` — instead of an OS
   error; the reported three identical OS-error lines imply the driver exposes ES2 and also failed at
   the DC level.)

5. **Mirror comparison — eframe passes the handle.** `eframe-0.37.0/src/native/glow_integration.rs:1270-1276`
   builds `glutin_raw_window_handle` from the created window and feeds it to
   `context_attributes_to_try` (`:1870-1883`, all three `build(raw_window_handle)`). `gl_boot.rs` deviates
   by `build(None)`; this is the precise delta.

6. **Error code decoding.** 0xC007000D is not any documented Win32/GL constant: MinGW `winerror.h`
   defines `ERROR_INVALID_PIXEL_FORMAT`=2000, `ERROR_BAD_DRIVER`=2001, `ERROR_INVALID_WINDOW_STYLE`=2002
   (`/usr/x86_64-w64-mingw32/include/winerror.h:1360-1362`) — none match; a header grep for
   `0xC007000D` finds nothing. GitHub issue search: `"c007000d"` → 0 results; `"-1073283059"` → one
   unrelated servo Win7 build issue (servo/servo#23538). Interpretation: an undocumented internal
   failure code set by opengl32.dll / win32u.sys inside the failed WGL creation call; the message-less
   code (FormatMessage error 317) is characteristic of opengl32-internal values. Sandboxed
   `web_search` was failing (DDGSException), so this is doc-fetch + GitHub-API based
   (Microsoft Learn `wglCreateContext`; `api.github.com/search/issues`).

### Secondary suspects examined and ruled out

- `with_content_protected(true)` → `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`
  (`winit-0.30.13/src/platform_impl/windows/window.rs:1029-1035`) — capture affinity only, does not
  affect GL rendering.
- winit window class registers `CS_HREDRAW | CS_VREDRAW` only
  (`winit-0.30.13/src/platform_impl/windows/window.rs:1409-1429`) — eframe works on Windows with this
  class, so no `CS_OWNDC`/`CS_GL_CONTEXT_INITIALIZATION` requirement is violated.
- Transparency path (`winit window.rs:1231-1244`, DWM blur-behind with empty region) runs *after*
  window creation and is unrelated to context creation.
- Event-loop-on-non-main-thread: `EventLoopBuilderExtWindows::with_any_thread(true)` is set
  (`crates/overlay/src/service.rs:83-89`); the window flash confirms winit creation succeeded.

### Ranked fix options (no new dependencies)

1. **Pass the window's raw handle into the context attributes** —
   `crates/overlay/src/gl_boot.rs`: change `create_context(display, config)` to take `&Window` and use
   `builder.build(Some(window.window_handle().as_raw()))` (`:221`), mirroring
   eframe `glow_integration.rs:1270-1276`. glutin then calls `apply_on_native_window`
   (= `SetPixelFormat` with the chosen ARB pixel-format index, NULL descriptor) before the creation
   call (`api/wgl/context.rs:36-38`), and the context's pixel format matches the surface DC created
   later (`api/wgl/surface.rs:89-104` also calls `apply_on_native_window`; without this, even a lucky
   creation would fail at `wglMakeCurrent` on format mismatch). One-line change + fix the wrong
   comment at `gl_boot.rs:212-213`. Cheapest, no new deps.
2. **Keep `build(None)` but call `config.apply_on_native_window(&handle)` manually before
   `create_context`** (public unsafe, `api/wgl/config.rs:295-310`). Equivalent effect, less idiomatic;
   still needs the handle for later `MakeCurrent` format consistency, so option 1 is preferred.
3. **If a driver still rejects Core 3.3 after fix 1**: reorder/relax the attempt list (Compatibility
   first, or drop the GLES attempt). Config-only, no deps — speculative fallback.
4. **ANGLE via `ApiPreference::PreferEgl`** — requires shipping `libEGL.dll`/`libGLESv2.dll` = **new
   native dependency → needs explicit user approval**. Only justified if system WGL still fails after
   options 1-3. Note `FallbackEgl` already falls back to EGL only when WGL display creation fails,
   and Windows has no system `libEGL.dll`.
5. **Dropping alpha/transparency from the config template** — rejected: config selection already
   succeeded; the failure is DC-state, not config attributes.

### Confidence and follow-up verification

Confidence: **high (~0.85)** for the root cause (glutin's own control flow + docs make the
handle-less `build(None)` → no-`SetPixelFormat` → WGL creation failure the only path that produces
this symptom shape); **medium** on the exact semantics of 0xC007000D (undocumented code).

A follow-up Windows run should verify:
- overlay_demo.exe creates a context (which attempt succeeds: Core 3.3 vs Compatibility — log line 226
  no longer printed);
- transparent blending actually composites (DWM blur-behind + 8-bit alpha), `swap_buffers`/resize OK;
- optional debug print of the chosen pixel format (`wglGetPixelFormatAttributePINDEXARB`
  `iAlphaBits=8`, `TRANSPARENT_ARB=1`) confirms the ARB format applied to the window DC;
- the host-backend path (`OverlayHost` on eframe's loop) was never affected — eframe passes the handle.

## Stage 6 research — Windows chip rendering via OverlayHost (B) — user-provided external answer
Source: user asked elsewhere; answer saved in .tmp/overlay-fixes-notes.md (2026-11-04 20:39).
Hypotheses listed there:
1. WS_EX_LAYERED vs CS_OWNDC conflict (Microsoft forbids layered windows on CS_OWNDC
   window classes; winit historically sets CS_OWNDC).
2. Transparent child viewports with glow on Windows become opaque unless the chosen GL
   config advertises transparency; fix PR egui#8423 merged in egui_glow 0.36.2 (we are on
   0.37.0 — must verify the code path is actually present/enabled).
3. Windows does not deliver RedrawRequested for "invisible" windows; eframe has a
   forced-repaint workaround; if the chip viewport is classified invisible it may never repaint.
4. always_on_top + transparent bug: renders black full-screen rect while focused, OK when
   unfocused.
5. AMD driver-specific transparent failures (Intel/NVIDIA fine).
Recommended by the external answer: update deps (moot — 0.37 already > 0.36.2), disable
window shadow (ViewportBuilder::with_has_window_shadow(false) — egui docs tie this to
Windows transparency), override App::clear_color to fully transparent, avoid opaque
CentralPanel; fallback = separate winit window (thread backend) or egui_overlay crate
(new dep = approval needed).
To verify against vendored sources: which of these apply to egui/eframe 0.37 code we
actually use in crates/overlay/src/host.rs + toast_debug example (shadow flag, per-viewport
clear color, transparency handling on Windows).

## Stage 6 research (B) — hypothesis verification against vendored 0.37 code

Verified 2026-11-04 against `.cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`
(egui-0.37.0, eframe-0.37.0, egui-winit-0.37.0, egui_glow-0.37.0, glutin-winit-0.5.0,
winit-0.30.13). Citations below use that prefix. Scratch diff of egui#8423 kept in
`.tmp/pr8423.diff`.

### Root cause found in the vendored stack: `set_cursor_hittest(false)` ⇒ unattributed WS_EX_LAYERED

The invisible chip is caused by **click-through**, not by transparency:

- `crates/toast/src/service.rs:237-238` sets `transparent: true, click_through: true` for every
  chip; `crates/overlay/src/host.rs:287` maps that to `ViewportBuilder::with_mouse_passthrough(true)`.
- egui-winit applies passthrough at window creation via `window.set_cursor_hittest(false)`
  (`egui-winit-0.37.0/src/lib.rs:2492-2494`; command path `lib.rs:2153-2156`).
- winit Windows turns that into `WindowFlags::IGNORE_CURSOR_EVENT`
  (`winit-0.30.13/src/platform_impl/windows/window.rs:632-637`), which adds
  **`WS_EX_TRANSPARENT | WS_EX_LAYERED`**
  (`winit-0.30.13/src/platform_impl/windows/window_state.rs:300-302`, flag def :116).
- winit 0.30.13 **never calls `SetLayeredWindowAttributes`/`UpdateLayeredWindow`** anywhere in
  `src/` (ripgrep: zero hits). Win32 rule: a `WS_EX_LAYERED` window composites **nothing** until
  layered attributes are set — so the GL frame is swapped into a redirection bitmap DWM never
  presents. DWM still shows the window frame/shadow and the default (white) client surface —
  exactly the reported symptom ("window exists, white opaque client area + shadow, content never
  painted").
- Linux works because X11 `set_cursor_hittest` is an input-shape region, not a layered window
  (`winit-0.30.13/src/platform_impl/linux/x11/window.rs` `set_cursor_hittest`), so painting is
  visible there.

### Verdicts on the external hypotheses (our exact versions)

1. **`with_transparent(true)` requires `with_has_window_shadow(false)` on Windows — FALSE.**
   egui 0.37's field is `has_shadow` (`egui-0.37.0/src/viewport.rs:324`, `with_has_shadow`
   :503-519); its docs tie ghosting to **macOS only** (:428). egui-winit discards it on Windows:
   `has_shadow: _has_shadow` (`egui-winit-0.37.0/src/lib.rs:2262`); winit 0.30.13 has no shadow
   API. `with_transparent(true)` → `WindowAttributes::with_transparent` (:2289) →
   `WindowFlags::TRANSPARENT` (`winit .../windows/window.rs:1325`, `set_transparent` :122-129),
   but `to_window_styles()` **never maps TRANSPARENT to any style** (window_state.rs:287-306);
   its only Windows effect is a no-op DWM blur enable with an empty region
   (`windows/window.rs:1232-1246`). The shadow is not painting a white rectangle — the white is
   the never-composited redirection surface.
2. **egui#8423 — PRESENT in eframe 0.37.0.** PR merged 2026-08-24 (GitHub API, merged=True,
   base main); the vendored `eframe-0.37.0/src/native/glow_integration.rs:206-226` contains the
   Windows branch (transparent windows created via `event_loop.create_window`, bypassing
   `glutin_winit::finalize_window`, which strips transparency when the GL config reports no alpha
   — `glutin-winit-0.5.0/src/lib.rs:196-202`), plus child inheritance of root `transparent`
   (glow_integration.rs:1626-1635). Caveat: the fix only keeps the *attribute*; winit Windows
   still does nothing with it (see 1), so transparency remains "a hint"
   (`egui-0.37.0/src/viewport.rs:418-421`). Clear color: **one app-level clear for ALL viewports**
   (`glow_integration.rs:763-765` `app.clear_color(&visuals)`, `painter.clear` :876-878; also
   `clear_before_update` multi-viewport hack :771) — no separate viewport clear path. eframe
   default is `rgba(12,12,12,180)` (`eframe-0.37.0/src/epi.rs:251-256`) → `toast_debug` shows a
   dark-gray fill; `src/gui.rs:1053-1059` already overrides to alpha-0 for virshot.
3. **Invisible-window repaint — TRUE mechanism, already handled upstream; our sequencing is
   correct.** eframe 0.37 documents "invisible windows don't receive `RedrawRequested` on
   Windows" (`winit_integration.rs:12-17`) and forces repaints: `check_redraw_requests` runs
   invisible windows directly at `PassMode::Full` (`run.rs:217-249`, throttle 100 ms :22-29),
   Windows `RepaintNow` fix (:131-140). `RequestPaintWhileHidden` flows
   egui → `ActionRequested::PaintWhileHidden` (`egui-winit lib.rs:1913-1914, 2161-2163`) and is
   honored as `visible || wants_paint` (`glow_integration.rs:668-673`). `host.rs` does the right
   dance: hidden spawn (:286), `request_repaint_of` gated on `raw.viewports` (:295-304),
   `RequestPaintWhileHidden` every pass (:300-302), `Visible(true)` after first paint (:305-312),
   callback sets `painted` (:316-341). Repaint sequencing is **not** the Windows bug.
4. **Flags we set today + paint plumbing.** `host.rs:284-289`: transparent, decorations=false,
   always_on_top, mouse_passthrough, visible=false, active=false. `content.rs:130-146` paints a
   full-rect opaque chip fill + text. Textures/fonts: **one painter + one GL context shared by all
   viewports** (`glow_integration.rs:76-77`, :117-125), made current per viewport surface via
   `change_gl_context` (:1123-162 shape: :1123-1162; Windows `is_current` ignores the surface,
   hence the `current_viewport` tracking). Font texture sharing across deferred viewports is fine;
   not the bug.
5. **CS_OWNDC vs layered — FALSE for our stack.** winit 0.30.13 registers its window class with
   `CS_HREDRAW | CS_VREDRAW` and `hbrBackground: 0` only
   (`winit .../windows/event_loop.rs:911-928`) — **no CS_OWNDC**. winit does not refuse layered
   windows; it *sets* `WS_EX_LAYERED` itself for hittest=false (see root cause). The Microsoft
   CS_OWNDC/layered conflict does not apply to winit 0.30.13.

### Concrete minimal fix set (cheapest first, no new deps)

1. **Windows: drop mouse passthrough on the host backend.**
   `crates/overlay/src/host.rs:287` → `with_mouse_passthrough(spec.click_through && !cfg!(windows))`.
   Removes `WS_EX_LAYERED` → the chip window composites normally; chip is never interactive, so
   click-through loss is invisible to users (only the chip rect can take mouse hits). Fixes both
   `toast_debug` and virshot with one line, since both spawn chips through the same host path
   (`src/gui.rs:1184-1190` + `crates/toast/src/service.rs:233-238`). Cheapest Windows A/B proof.
2. **Keep click-through by attributing the layered window: `SetLayeredWindowAttributes(hwnd, 0, 255,
   LWA_ALPHA)` after creation.** Precedents exist: hand-written DLL FFI with zero crates.io deps
   (`crates/gamepad`) and the module-scoped `unsafe_code` allow in `crates/overlay/src/gl_boot.rs`.
   Plumbing constraint: child windows are created inside eframe (`initialize_or_update_viewport`),
   and host.rs gets only a `ViewportId` — no window handle — so this is only reachable where we own
   the winit `Window`: the **thread backend** (`service`/`gl_boot`, standalone processes/examples)
   or an eframe patch (fork/git dep ⇒ needs user approval). Not applicable to virshot while it
   hosts eframe.
3. **`toast_debug` clear-color parity:** override `App::clear_color` to
   `egui::Color32::TRANSPARENT.to_normalized_gamma_f32()` like `src/gui.rs:1053-1059`
   (`crates/toast/examples/toast_debug.rs` currently uses eframe's default alpha-180 fill).
   Cosmetic until the window composites; matters once Windows transparency ever works.
4. **`with_has_window_shadow(false)` — skip.** No-op on Windows in 0.37 (macOS-only flag,
   discarded by egui-winit; winit has no shadow API).
5. Thread-backend fallback for Windows-only toast (no eframe host): the same fix-2 call inside
   `gl_boot`/`service` window creation gives real click-through overlays there.

### Does virshot inherit?

Yes — `virshot` paints toasts through the identical `OverlayHost::update` child-viewport path
(`src/gui.rs:1171-1190`, root viewport already `.with_transparent(true)` so the alpha-capable GL
config is selected), so fix 1 fixes virshot automatically once host.rs is gated; its clear color
is already alpha-0.

### What only a Windows re-run can settle

- Exact visual behavior of an unattributed `WS_EX_LAYERED` window on the user's Windows build
  (white redirection surface + shadow as observed vs fully invisible) — symptom matches the
  chain above, but cannot be reproduced in this Linux sandbox.
- Fix-1 confirmation: chip becomes visible on Windows with `with_mouse_passthrough(false)`.
- Whether `SetLayeredWindowAttributes(…, LWA_ALPHA=255)` preserves per-pixel alpha from the
  redirection bitmap with the chosen WGL config (chip fill is opaque full-rect, so mostly moot),
  and whether `WS_EX_TRANSPARENT` passthrough still works once attributes are set.
- What `gl_config.supports_transparency()` reports on the user's driver (only affects the 8423
  error-log branch) and whether the pixel format has alpha bits.
- Whether `ViewportCommand::ContentProtected` keeps working on the re-composited window.

## Stage 6 research (C) — all-monitors fan-out

Question: why does the toast NOT show on all monitors even with the "all monitors"
checkbox, and what is the minimal fix? Verdict: **the library fan-out is correct;
the debug example enumerates exactly ONE monitor rect.** `MonitorSel::All` fans
out over whatever rects the service was spawned with — with one rect it is one
chip, on every OS.

### 1. Library fan-out (verified, not a bug)

- `ToastService::spawn(sink, monitors: Vec<Rect>, wake)` — service.rs:97: the
  timer thread owns `monitors` for its whole life.
- `show()` — service.rs:222-250: `for rect in placements(monitors, toast.monitor,
  toast.corner, size)` (service.rs:232) → ONE `sink.spawn_window(spec, ...)` per
  rect (service.rs:243). Empty list logs and skips (service.rs:223-228, :248-251).
- `MonitorSel::All` = every rect: placement.rs:22 (`MonitorSel::All => monitors.to_vec()`);
  `Index(n)` = one rect (placement.rs:23-26); chip too big for a monitor → skipped
  (placement.rs:30, fits() placement.rs:36-38).
- `MonitorSel::All` is the default and `Toast::text/image` set it (toast.rs:16-17, :47, :61).
- Unit tests prove N-rect fan-out: service.rs:512
  `show_spawns_one_chip_per_monitor_at_full_alpha` asserts 2 spawns for 2 rects
  (incl. second-monitor offset); placement.rs:59-70 asserts chips placed on both
  rects. Drop hides all ids (service.rs:123-146).
- Host backend places each child viewport at the spec rect: overlay host.rs:266-268
  (`ViewportBuilder::with_position(spec.rect.x, spec.rect.y)`), so 2 rects = 2
  windows at 2 positions — confirmed empirically below (not just by tests).

### 2. Where `monitors` come from — two feeders, one healthy, one broken

- **virshot (healthy)**: `run()` builds the stack at gui.rs:1183-1187 —
  `monitor_rects_logical(cc.egui_ctx.pixels_per_point())` (gui.rs:1102-1138) via
  `xcap::Monitor::all()` (gui.rs:1104), physical px ÷ scale, PRIMARY first
  (gui.rs:1121-1136), then `host.set_monitors` + `ToastService::spawn(monitors…)`.
  xcap enumerates ALL monitors: Windows `EnumDisplayMonitors`
  (xcap-0.9.8 src/windows/impl_monitor.rs:156-176); Linux/X11 XRandR `GetMonitors`
  per output (xcap-0.9.8 src/linux/impl_monitor.rs:202-225); Wayland via
  libwayshot outputs (utils `wayland_detect`). Checkbox wiring:
  `monitor_sel(on_all_monitors)` → `MonitorSel::All`/`Index(0)` (src/capture.rs:195-200),
  applied per shot (src/capture.rs:233). So virshot with 2+ monitors spawns N chips;
  it does NOT share the example bug.
- **toast_debug (broken enumeration)**: `host_monitor(ctx)`
  (crates/toast/examples/toast_debug.rs:87-102) returns exactly ONE rect from
  `ctx.input(|i| i.viewport().monitor_size)` (fallback inner_rect, fallback 800x600),
  hardcoded origin (0,0) — wired at toast_debug.rs:126/:131. UI prints it at
  toast_debug.rs:180; checkbox at toast_debug.rs:206. egui's
  `ViewportInfo::monitor_size` is the monitor of the CURRENT viewport only
  (egui-0.37.0 src/data/input/viewport_info.rs:45-46), filled by egui-winit from
  `window.current_monitor()` in logical points (egui-winit-0.37.0 src/lib.rs:1485-1492).
  So the user's Windows UI `[{x:0,y:0,w:1680,h:1050}]` = primary monitor ÷ scale —
  structurally impossible to ever show >1 rect, on Linux too.

### 3. Can eframe enumerate monitors? (vendored 0.37.0, verified)

- egui: no monitor-list API — grep of egui src finds `MonitorHandle`/`available_monitors`
  only in fullscreen-index doc comments (viewport.rs:345, :703, :1150).
- egui-winit: uses `event_loop.available_monitors()` INTERNALLY only
  (src/lib.rs:2067, :2225; src/window_settings.rs:250); nothing public enumerates.
- eframe: `native/event_loop_context.rs` (`with_current_event_loop`) is a PRIVATE
  module (`native/mod.rs:3 mod event_loop_context;`) — NOT reachable from an App.
- BUT: `CreationContext::winit_window()` (eframe src/epi.rs:141-145) and
  `Frame::winit_window()` (epi.rs:815-820) expose `&Arc<winit::window::Window>`;
  `Window::available_monitors()` gives all `MonitorHandle`s
  (winit-0.30.13 src/monitor.rs: size :118, position :125, scale_factor :154).
  Types are inferred — NO new dependency and no winit import needed (verified by
  compiling; toast manifest unchanged).

### 4. Repro evidence (./.tmp/stage6-c-repro/, Xvfb, host backend)

Xvfb multi-screen/Xinerama merge is impossible in this sandbox (`Xvfb -screen 0 …
-screen 1 … +xinerama` creates only socket `X99`; `:99.1` unopenable; root stays
1280x1024). Used the sanctioned fallback: single Xvfb screen **2560x1024x24** +
temporary patches (both reverted; git clean of code changes at finish).

- Baseline, unpatched (`wide-base-stderr.txt`, `wide-base-tree.txt`, `wide-root.png`):
  ONE chip — `spawn id=OverlayId(1) rect=(2310,928,220,36)` (bottom-right of the
  WHOLE 2560-wide virtual screen, i.e. monitors = [single monitor_size rect]);
  `xwininfo -root -tree`: `0x200009 "toast_debug" 220x36+2310+928` + root 800x600.
  Pixel crops (imagemagick `%[mean]`): left-half chip slot (1030,928) = 0,
  right chip slot (2310,928) = 6189 → exactly one visible chip, far right.
  (Single 1280x1024 run, `baseline-stderr.txt`: same, `rect=(1030,928,220,36)`.)
- TEMP patch 1 (synthetic `[(0,0,1280,1024),(1280,0,1280,1024)]`)
  (`wide-patched-stderr.txt`, `wide-patched-tree.txt`, `wide-patched-root.png`):
  TWO spawns — `rect=(1030,928,220,36)` and `rect=(2310,928,220,36)`; tree shows
  chips `+1030+928` AND `+2310+928`; crop means: patched@1030 = 6189 vs base@1030 = 0
  → **the service fans out N chips on N rects; the enumeration is the bug.**
- TEMP patch 2 (the proposed fix, enumerate via `cc.winit_window().available_monitors()`):
  compiles, `cargo clippy -p toast --all-targets -- -D warnings` clean, no manifest
  change; run under single-monitor Xvfb (`wide-fix-*`) gives identical single chip
  `rect=(2310,928,220,36)` — correct degenerate behavior; reverted.

### 5. Root cause verdict

(a) **Example enumeration primary-only — CONFIRMED**: `host_monitor` yields one
rect by construction on Windows AND Linux; "all monitors" checkbox only selects
`MonitorSel::All`, which fans out over that 1-entry list → 1 chip.
(b) **Library fan-out bug — NO**: verified N chips for N rects (tests + Xvfb
window tree + screenshots). virshot already shows on all monitors via xcap
(Win: EnumDisplayMonitors; Linux/X11: RandR outputs; Wayland: wayshot outputs).

### 6. Minimal fix (ranked, no new deps)

1. **toast_debug: enumerate via `cc.winit_window().available_monitors()`** in
   `ToastDebugApp::new`, ÷ `ctx.pixels_per_point()`, fallback to the old
   monitor_size path when `None`/empty (verified compile+clippy+run; zero new
   crates, same shape as virshot gui.rs:1102):
   ```rust
   fn host_monitor(cc: &eframe::CreationContext<'_>) -> Vec<Rect> {
       let scale = cc.egui_ctx.pixels_per_point().max(1.0) as f64;
       let mut rects = Vec::new();
       if let Some(window) = cc.winit_window() {
           for monitor in window.available_monitors() {
               let pos = monitor.position();
               let size = monitor.size();
               rects.push(Rect::new(pos.x as f64 / scale, pos.y as f64 / scale,
                   size.width as f64 / scale, size.height as f64 / scale));
           }
       }
       if !rects.is_empty() { return rects; }
       // old viewport().monitor_size fallback (unchanged)
   }
   ```
2. xcap as a dev-dep of crates/toast — parity with virshot's path, but adds a
   manifest dependency (needs user flag per AGENTS.md); unnecessary given (1).
3. Dead ends (verified): eframe's `event_loop_context` is private →
   `event_loop.available_monitors()` NOT reachable from an App; egui ctx has no
   monitor list.

Follow-up caveat (NOT the root cause): mixed-DPI Windows. Both virshot
(gui.rs:1102-1129) and fix (1) divide every physical monitor rect by the ROOT
`pixels_per_point`; with per-monitor DPI the chip on a secondary monitor lands at
a wrong position (possibly off that monitor). A refined version could scale per
monitor via `monitor.scale_factor()` (winit MonitorHandle :154) — separate
decision, not needed for "all monitors" fan-out.

## Stage 7 research (E) — taskbar-hidden overlay windows

Question: hide overlay/toast chip windows from the Windows taskbar with the locked stack
(winit 0.30.13, egui/eframe/egui-winit/egui_glow 0.37, glutin 0.32, raw-window-handle 0.6.2;
no new crates.io deps). Main eframe window may stay visible.

Registry root shorthand below: `REG = .cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.

### 1. How the taskbar entry is decided (winit 0.30.13, Windows)

- Default: `REG/winit-0.30.13/src/platform_impl/windows/window.rs:1339` — when no owner is set,
  `WindowFlags::ON_TASKBAR` is set → `window_state.rs:273-275` adds `WS_EX_APPWINDOW`. So every
  plain winit window gets a taskbar button unless something removes it.
- Removal mechanism winit ships = `ITaskbarList::DeleteTab`, NOT `WS_EX_TOOLWINDOW`:
  `platform_impl/windows/window.rs:1498` `pub(crate) unsafe fn set_skip_taskbar(hwnd, skip)`
  (CoCreateInstance CLSID_TaskbarList; silently no-ops if creation/HrInit fails, :1508-1516).
  Applied at creation in `on_create` (`window.rs:1249`) before the window is shown, and
  re-applied on the `TaskbarCreated` broadcast after explorer restarts
  (`platform_impl/windows/event_loop.rs:2454-2457`).
- Public builder knob: `platform/windows.rs:454` `pub trait WindowAttributesExtWindows`,
  `:500 fn with_skip_taskbar(self, skip: bool)` (impl for `WindowAttributes` at :540/:572).
  `:486 fn with_taskbar_icon(Option<Icon>)` only sets the icon bitmap (`ICON_BIG`,
  `platform_impl/windows/window.rs:902-908`) — it never hides the entry.
- Runtime knob (safe): `platform/windows.rs:254` `pub trait WindowExtWindows`,
  `:274 fn set_skip_taskbar(&self, skip: bool)`, implemented for `winit::window::Window`
  at :362/:373. Note `set_visible` (:135) does NOT re-apply skip_taskbar — only
  `TaskbarCreated` does (:2454).
- Reliability evidence from winit itself: `event_loop.rs:932-940` uses `WS_EX_TOOLWINDOW` for
  its internal message-only window and states that without it the window shows in the taskbar
  "initially, but it can show up at some later point" (after hours, or after killing/restarting
  explorer). I.e. DeleteTab alone is a best-effort hint; TOOLWINDOW is the never-show style.
- Raw handle: `Window::window_handle()` (`src/window.rs:1626`, HasWindowHandle) returns rwh 0.6
  `Win32WindowHandle { hwnd: NonZeroIsize, hinstance: Option<NonZeroIsize> }`
  (`REG/raw-window-handle-0.6.2/src/windows.rs:52-57`); Windows-only safe any-thread variant
  `WindowExtWindows::window_handle_any_thread` (`platform/windows.rs:356`).
- Window class: `platform_impl/windows/mod.rs:51` default class_name = `"Window Class"`
  (overridable via `with_class_name`, `platform/windows.rs:503`); registered at
  `platform_impl/windows/window.rs:1309-1310/1409-1425` — usable for EnumWindows matching.
- Parent-aggregation knob: `src/window.rs:475` `pub unsafe fn with_parent_window(RawWindowHandle)`
  (GWLP_HWNDPARENT) — builder-time only, groups the child under the parent's button (still in
  Alt-Tab); not a hide.

### 2. egui 0.37 ViewportBuilder: `taskbar` (presence), not `taskbar_icon` (bitmap)

- `REG/egui-0.37.0/src/viewport.rs:328` `pub taskbar: Option<bool>` (Windows section);
  `:525 fn with_taskbar(show: bool)`. Default `ViewportBuilder::default()` leaves it `None`
  → egui-winit passes nothing → winit default (button shown). This is exactly why the 3
  child-viewport chips appear as "egui window" entries.
- egui-winit mapping (`REG/egui-winit-0.37.0/src/lib.rs`): `create_winit_window_attributes`
  (:2238) destructures `taskbar: _taskbar` (:2273); default child title
  `"egui window"` (:2293); `#[cfg(target_os = "windows")]` block (:2443-2450):
  `if let Some(show) = _taskbar { window_attributes.with_skip_taskbar(!show) }`.
  So `ViewportBuilder::with_taskbar(false)` ⇒ `skip_taskbar(true)` ⇒ DeleteTab at creation.
  The field is Windows-gated: on Linux/macOS it is ignored (no repo-side cfg needed).
- Post-creation changes: the `ViewportCommand` enum (`egui viewport.rs:1085`) has NO taskbar
  variant; `apply_viewport_builder_to_window` (egui-winit lib.rs:2487) handles only
  mouse_passthrough/size/position. A taskbar value change via `ViewportBuilder::patch`
  (`egui viewport.rs:718`) sets `recreate_window = true` (:921-924) → eframe drops and
  recreates the window (glow_integration.rs:1661-1675). Constant builder values ⇒ no recreate.
- `taskbar_icon` does not exist on ViewportBuilder; the window icon travels via `icon`/
  `ViewportCommand::Icon` (egui-winit lib.rs:2091 → winit `set_taskbar_icon`) and only changes
  the bitmap, never presence.

### 3. Host backend reachability (OverlayHost) — windows are NOT exposed

- eframe 0.37 owns child windows in `struct Viewport { window: Option<Arc<Window>>, ... }`
  (`REG/eframe-0.37.0/src/native/glow_integration.rs:134-152`) inside `App.viewports`
  (:127-130) — all private; no public accessor anywhere (egui-winit `State` never stores
  windows either). `egui_winit::State` only borrows `&Window` per call.
- Public handle access is root-only: `Context::winit_window()` (`eframe/src/epi.rs:144`) and
  `AppDraft::winit_window()` (:819); `NativeOptions.window_builder` hook (`epi.rs:42/:363`)
  applies to the root window only.
- Creation timing: `initialize_all_windows` (glow_integration.rs:1356) runs at Resume (:543)
  and at the end of every redraw pass (:1601), i.e. right AFTER `App::ui` returns; window
  creation = `create_window_for_viewport` (:195-227) → `egui_winit::create_winit_window_attributes`.
  Repo `crates/overlay/src/host.rs:264-282`: builder is rebuilt every pass; the pass that
  registers the viewport is the pass that creates the window. Chips are created hidden
  (`with_visible(false)`, host.rs:281) and shown later via `ViewportCommand::Visible(true)`
  (host.rs:318).
⇒ Conclusion: builder-time `with_taskbar(false)` is the ONLY safe path for the host backend;
runtime per-child hwnd access does not exist without OS-level heuristics (EnumWindows).

### 4. Thread backend reachability (crates/overlay/src/service.rs)

- `Backend::window_attributes` (`crates/overlay/src/gl_boot.rs:196-209`) builds
  `WindowAttributes::default()` — no skip_taskbar today; `WindowEntry` (service.rs) owns the
  `Window`, so both builder-time and runtime safe knobs are usable here
  (`window_handle().hwnd` also available for FFI).

### 5. Ranked options (cheapest first)

1. **Host backend, zero unsafe:** add `.with_taskbar(false)` to the viewport builder in
   `crates/overlay/src/host.rs:266-282`. Effects: egui-winit lib.rs:2447-2450 → winit
   `with_skip_taskbar(true)` → DeleteTab in `on_create` before show. Windows-only by
   construction (egui-winit cfg); no repo cfg needed; main window untouched (gui.rs:1167
   keeps default). Caveat: DeleteTab is best-effort (winit's own comment event_loop.rs:932-940);
   a chip that gets *activated* (Windows click-through is disabled in host.rs:280, so chips can
   receive clicks) or an explorer restart could try to re-add a button; winit re-applies on
   `TaskbarCreated` (:2454) but NOT on later `Visible(true)`.
2. **Thread backend, zero unsafe:** `.with_skip_taskbar(true)` via
   `winit::platform::windows::WindowAttributesExtWindows` in `gl_boot.rs::window_attributes`
   (cfg(windows), gl_boot.rs:196-209). Same DeleteTab caveat as (1).
3. **Runtime re-assert (thread backend only, zero unsafe):** `WindowExtWindows::set_skip_taskbar(true)`
   on owned windows (platform/windows.rs:274, impl :373), e.g. after first paint/show, to
   re-delete a button that reappeared on show. Not available for host children (no handle).
4. **FFI fallback, `WS_EX_TOOLWINDOW` (the never-show style)** — needed only if Windows
   verification shows buttons reappearing:
   - (a) thread backend: `window_handle()` → `Win32WindowHandle.hwnd` (rwh 0.6.2 windows.rs:52-57)
     → `GetWindowLongPtrW(GWL_EXSTYLE)` | `WS_EX_TOOLWINDOW` → `SetWindowLongPtrW` (user32.dll
     extern, gameinput.dll-shim precedent `crates/gamepad/src/gameinput.rs`).
   - (b) host backend WITHOUT hwnd: `EnumWindows` + `GetClassNameW == "Window Class"`
     (winit default class, platform_impl/windows/mod.rs:51) + title match (default child title
     "egui window", egui-winit lib.rs:2293; better: give OverlayHost children a unique
     `.with_title("virshot-overlay")` so matching is deterministic) +
     `GetWindowLongPtrW(GW_OWNER) == 0`, then add `WS_EX_TOOLWINDOW`. Timing works: scan in an
     update pass after creation (window exists at glow_integration.rs:1601 and is still hidden
     — host.rs:281 Visible(false), show comes later at :318), so TOOLWINDOW lands BEFORE the
     window ever shows ⇒ no taskbar flash; dedupe by hwnd (one-shot).
   - Unsafe placement: new module `crates/overlay/src/win_taskbar.rs` with module-scoped
     `#![allow(unsafe_code)]` (exact shape of gl_boot.rs:17 and gameinput.rs:25 under overlay's
     root `#![deny(unsafe_code)]`, lib.rs:36). `crates/toast` (:20 forbid) and virshot
     (src/main.rs:63 forbid) stay clean — all unsafe confined to overlay. Requires the AGENTS.md
     Safety exception text and overlay lib.rs doc to be widened from "gl_boot only" to
     "gl_boot + win_taskbar" = spec/docs change — FLAG for user approval before code lands.
5. **Rejected:** `GWLP_HWNDPARENT` = main hwnd (main reachable via `cc.winit_window().window_handle()`)
   — groups children under the main button instead of hiding them (still Alt-Tab-visible);
   winit's builder `with_parent_window` (window.rs:475, `unsafe`) cannot be injected for
   eframe-created children anyway.

### 6. Linux side — fix stays Windows-only

- Neither backend sets `override_redirect` anywhere (grep: not in gl_boot.rs, host.rs, service.rs)
  → on the Linux dev host chips/overlays are WM-managed normal windows → they DO appear in
  GNOME/X11 window lists (dock). On Wayland there is no escape hatch: winit's `override_redirect`
  field (`platform_impl/linux/mod.rs:86`) is consumed only by the X11 backend
  (`platform_impl/linux/x11/window.rs:277-278` sets `CreateWindowAux.override_redirect`; the
  WM ignores override-redirect windows → no dock entry — winit itself uses this for its hidden
  activation window, `x11/activation.rs:109`); the Wayland backend ignores it
  (`platform/x11.rs:162` is the X11-only ext trait).
- Product ships Windows (AGENTS.md); Linux is dev-host only ⇒ apply the fix Windows-only.
  egui's `with_taskbar(false)` is already cfg-gated to Windows inside egui-winit, so repo code
  needs no cfg at all. Optional future nicety for dev hosts:
  `ViewportBuilder::with_override_redirect(true)` (egui viewport.rs:696 → egui-winit lib.rs:2436-2438
  under `cfg(all(feature = "x11", target_os = "linux"))`, enabled via Cargo.toml:26) — tradeoff:
  unmanaged windows lose WM placement/stacking/focus guarantees, may render behind managed
  windows; not worth it now.

### 7. Unsafe policy decision (summary)

- Options 1-3: ZERO unsafe — no policy change needed; gl_boot.rs stays the sole exception;
  toast/virshot stay `#![forbid(unsafe_code)]`.
- Option 4 adds unsafe: confine to `crates/overlay` only (new module with module-scoped allow,
  same shape as gl_boot/gameinput), update AGENTS.md Safety exception wording + overlay
  `lib.rs:25-36` doc comment — a docs/spec change that needs explicit user sign-off.

### 8. Windows verification checklist (must confirm after implementing 1+2)

1. `./cargo.sh build --release --target x86_64-pc-windows-gnu`, run on Windows.
2. Settings main window keeps its taskbar entry; chip windows ("egui window" / overlay titles)
   show NO taskbar button: (a) right after creation, (b) after `Visible(true)` shows a chip,
   (c) after clicking a chip (activation attempt — Windows chips are not click-through),
   (d) after killing/restarting `explorer.exe` mid-run (TaskbarCreated path),
   (e) over a long session (winit's "hours later" note targets DeleteTab-only windows).
3. Thread-backend example: overlay windows absent from taskbar.
4. If a button reappears in any case ⇒ escalate to option 4 (win_taskbar.rs TOOLWINDOW module,
   with the flagged AGENTS.md exception update).

## Stage 7 research (F) — chip placement units on multi-monitor Windows

Question: why `OverlayHost` child-viewport chips appear on monitors 2/3 at wrong positions and sizes
on the user's Windows machine (mixed DPI), and what the correct coordinate conversion is. Research-only:
no code changed. Evidence: `.tmp/stage7/windows_screenshot.png` (+ derived crops `.tmp/stage7/win{A,B,C}.png`,
`crop_windows.png`, `crop_monitor3.png`) and the scenario notes `.tmp/toast-monitors.md`.
Registry root shorthand: `REG = .cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.

### 1. Verdict (🅱 primary, 🅲 secondary, 🅰 ruled out)

- **🅱 — wrong units, CONFIRMED as the placement bug.** The caller divides monitor rects by the
  **root** `pixels_per_point` (and at app creation that value is still `1.0`), while egui multiplies
  the child viewport's position/size by the **child window's own** scale factor. Two independent
  conversions that cancel only when every monitor shares the root DPI.
- **🅲 — Windows child-viewport presentation, a SEPARATE defect, still open.** Screenshot shows three
  child windows titled `egui window`: two bodies are Windows 11 "ghost" placeholder cards (white
  gradient + client-area title text + generic icon = content never presented), one is a black
  chip-shaped bar (aspect 5.7 ≈ chip aspect 6.1, mid-height, not a corner) = painted but composited
  without alpha. Ghost bodies have aspect ≈2.6, NOT chip aspect ⇒ their size/position never reached
  the chip shape ⇒ 🅱 holds independently of 🅲.
- **🅰 — enumeration timing, ruled out:** three child windows exist, so monitors were enumerated and
  fan-out ran (`ToastService::show` spawns one window per placement rect, `crates/toast/src/service.rs:222-250`).

### 2. Exact conversion chain (verified against vendored sources)

1. `OverlayHost::paint` feeds the rect into the builder as **egui logical points**:
   `crates/overlay/src/host.rs:266-282` (`with_inner_size([rect.w, rect.h])`, `with_position(rect.x, rect.y)`).
2. Registration creates the window: `REG/eframe-0.37.0/src/native/glow_integration.rs:1356-1366`
   (`initialize_all_windows`, end of every pass) → `create_window_for_viewport` (:195-227).
   `egui_winit::create_winit_window_attributes` passes builder values to winit as
   `LogicalSize/LogicalPosition` scaled by **`zoom_factor` ONLY**
   (`REG/egui-winit-0.37.0/src/lib.rs:2324-2352`) — deliberately no monitor scale, because the monitor
   is unknown at that moment (egui#7095 / winit#4266).
3. winit picks the window's scale at creation: `REG/winit-0.30.13/src/platform_impl/windows/window.rs:1128-1148`
   — `CreateWindowExW` first, then `hwnd_dpi(window)` (`GetDpiForWindow`, `windows/dpi.rs:107-131`)
   → `dpi_to_scale_factor(dpi) = dpi/96` (`dpi.rs:76-79`) stored in `WindowState` **before** any later
   positioning. So the child's initial scale = DPI of the monitor its *initial* position lands on
   (primary when the position is bogus). Process is `PER_MONITOR_AWARE_V2` (`dpi.rs:32-52`).
4. The multiply that bites: eframe calls `egui_winit::apply_viewport_builder_to_window`
   (`glow_integration.rs:228`) → `REG/egui-winit-0.37.0/src/lib.rs:2484-2530`, comment :2496-2500
   ("we didn't know on what monitor the window would appear … Now we do know"):
   `ppp = pixels_per_point(egui_ctx, window) = ctx.zoom_factor() * window.scale_factor()`
   (`lib.rs:59-63`) → `request_inner_size(PhysicalSize::new(ppp*w, ppp*h))` (:2506-2511) and
   `set_outer_position(PhysicalPosition::new(ppp*x, ppp*y))` (:2529-2531). **Builder values are
   interpreted as logical points of the CHILD's monitor.**
5. Later passes: `process_viewport_commands` (`lib.rs:1952-2032`) applies
   `ViewportCommand::InnerSize(Vec2)` (:1969-1972) and `ViewportCommand::OuterPosition(Pos2)`
   (:2014-2018) with the same live `ppp` of that window ⇒ post-creation commands are an authoritative
   per-window correction (commands sent before a window exists are deferred:
   `glow_integration.rs:138-176`, repo gate `crates/overlay/src/host.rs:302-304`).
6. Per-viewport scale is observable: `update_viewport_info` (`lib.rs:1466-1496`) sets
   `native_pixels_per_point = window.scale_factor()` (:1496) and reports `inner_rect`/`outer_rect`
   in POINTS (:1430-1449) and `monitor_size` ÷ ppp (:1488); `WindowEvent::ScaleFactorChanged` updates
   that viewport's native ppp (`lib.rs:380-387`). All readable from
   `ctx.input(|i| i.raw.viewports.get(&viewport))` — public API, no new deps, no unsafe.
7. DPI change while a child moves monitors: `WM_DPICHANGED`
   (`REG/winit-0.30.13/src/platform_impl/windows/event_loop.rs:2216-2300`) early-returns when the scale
   is unchanged; otherwise the physical inner size is recomputed as
   `old_physical.to_logical(old_scale).to_physical(new_scale)` (:2274-2281) ⇒ **logical size preserved,
   physical scaled by `s_new/s_old`**; the OS-suggested rect is applied and then nudged toward the new
   monitor (:2349-2398). So a DPI transition preserves the chip's ASPECT but re-scales its physical
   size and shifts its position — which is why the ghost bodies (aspect ≈2.6) cannot be merely
   DPI-resized chips.
8. Per-monitor scale source: `MonitorHandle::scale_factor()` =
   `GetDpiForMonitor(MDT_EFFECTIVE_DPI)/96` (`windows/monitor.rs:206-207` via `dpi.rs:62-74`),
   `MonitorHandle::size()/position()` are **physical** (`monitor.rs:170-175`, :196-201). Root scale and
   the monitor list are reachable at creation via `cc.winit_window()` (`eframe/src/epi.rs:144`).
   Precedent: eframe itself converts the ROOT window with `egui_zoom_factor * monitor.scale_factor()`
   (`REG/eframe-0.37.0/src/native/epi_integration.rs:73`, :100-112) — never with `ctx.pixels_per_point()`.
9. `cc.egui_ctx.pixels_per_point()` at `CreationContext::new` is **1.0**: the root window is created
   before the app creator (`glow_integration.rs:330-431`) and `egui_winit::State::new` only stuffs
   `native_pixels_per_point` into the raw ROOT viewport input (`lib.rs:208-211`); egui computes ppp in
   `Context::begin_pass` (`REG/egui-0.37.0/src/context.rs:492-507`) and `Context::pixels_per_point()`
   reads that field (`context.rs:2712-2715`), whose default is `1.0`
   (`REG/egui-0.37.0/src/input_state/mod.rs:349`). ⇒ `crates/toast/examples/toast_debug.rs:91-111`
   and `src/gui.rs:1102-1138` (called with `cc.egui_ctx.pixels_per_point()` at `src/gui.rs:1183`)
   divide by **1.0**: physical pixels arrive at egui as un-scaled numbers, then step (4) multiplies
   them by the child monitor's scale.

### 3. Correct conversion (the one rule)

For a chip whose intended desktop rect is PHYSICAL `(x, y, w, h)` on monitor `k`, and a viewport whose
pixels-per-point is `ppp = zoom_factor * native_pixels_per_point`:

```
builder/command values = physical / ppp            // i.e. logical points OF THAT WINDOW
```

never `physical / root_ppp`, never "physical as logical". Position: `pos2(x/ppp, y/ppp)`; size:
`vec2(w/ppp, h/ppp)`. Chip art stays in logical design units (`CHIP_HEIGHT = 36.0` etc.,
`crates/toast/src/chip.rs`), so physical chip size = design × that monitor's scale = crisp and visually
consistent across monitors — but placement math (`overlay::place`, `CORNER_INSET_X/Y`, `fits()`) must run
in ONE space, and the thread backend already takes physical (`crates/overlay/src/gl_boot.rs:199-203`
`PhysicalSize/PhysicalPosition`). Hence: **placement space = physical desktop pixels**, conversion to
egui points happens only inside `OverlayHost`, at the two moments that know a real ppp.

Because the window's ppp is only known after creation, the initial builder value is a guess
(`physical / monitor.scale_factor()`), and the authoritative correction is the command pair
`OuterPosition` + `InnerSize` recomputed with the **observed** `native_pixels_per_point` of that
viewport — which also repairs the `WM_DPICHANGED` rect nudge (step 7).

### 4. Chosen fix — strategy (c): PHYSICAL rects end-to-end, host converts internally

- **API shape (KISS, minimal churn):** one small struct, no tuple ambiguity, no per-command scale:
  ```rust
  /// Monitor for placement, in physical desktop pixels, with that monitor's scale.
  pub struct MonitorRect { pub rect: Rect /* physical */, pub scale: f32 }
  impl OverlayHost { pub fn set_monitors(&self, Vec<MonitorRect>); pub fn monitors(&self) -> Vec<MonitorRect>; }
  ```
  exported from `crates/overlay/src/lib.rs` alongside `Rect`/`Corner`/`place` (:45-48).
  `Rect` and `place()` stay unchanged; `OverlayWindowSpec.rect` keeps its physical meaning for both
  backends (`lib.rs:74-79` doc becomes true for the host backend too).
- **`crates/toast`:** `placements(monitors, sel, corner, size)` (`src/placement.rs`) keeps pure geometry;
  `ToastService::show` computes the chip size per monitor as `chip_size(toast) * m.scale` so
  `fits()`/`place()` compare like-for-like physical numbers. Existing service tests
  (`Rect::new(0,0,1920,1080)` / `Rect::new(1920,0,1280,720)`) stay valid with `scale = 1.0`.
- **`OverlayHost::paint`:** builder gets `rect / (zoom * m.scale)` (best guess at creation,
  `ctx.zoom_factor()` for the zoom term since egui multiplies by `zoom * window.scale_factor()`);
  once the viewport has a window, read `raw.viewports[viewport].native_pixels_per_point` and, when it
  differs from the ppp last applied (stored per window in `PaintState`), send
  `ViewportCommand::OuterPosition(rect.min / ppp)` + `ViewportCommand::InnerSize(rect.size / ppp)`.
  Idempotent, self-healing across DPI changes and monitor moves; no new deps, no unsafe.
- **Callers:** `crates/toast/examples/toast_debug.rs:119-145` and `src/gui.rs:1102-1138/:1183` push
  `MonitorRect { rect: physical, scale: monitor.scale_factor() }` from
  `cc.winit_window().available_monitors()` (virshot should switch its placement path from `xcap`
  geometry to winit monitors — winit gives per-monitor DPI, xcap does not; xcap stays for capture).
  Fallback (no winit window / empty list): single `MonitorRect` from the viewport with `scale =
  cc.winit_window().map(|w| w.scale_factor()).unwrap_or(1.0)`.
- **Rejected alternatives:** (a) per-target logical rects at spawn time only — cannot repair the
  create-time DPI choice + `WM_DPICHANGED` position nudge, and keeps the current asymmetry where the
  thread backend wants physical and the host wants logical (one API, two units); (b) physical÷root ppp
  then commands after creation — the same root-scale error, only applied later. `ViewportBuilder::monitor`
  (`egui-winit lib.rs:2204-2236` → `Fullscreen::Borderless`) must NOT be used for chips: it makes the
  window borderless-fullscreen on that monitor. `clamp_size_to_monitor_size` (egui `viewport.rs:305-309`)
  defaults to false on Windows and chips are smaller than monitors ⇒ no interference.

### 5. Debug output design (cheap, dep-free)

`crates/overlay` stays dependency-free; its existing precedent is plain `eprintln!`
(`crates/overlay/src/host.rs:297`). Add a debug flag (`OverlayHost::set_debug(bool)`, default off — no env
var magic) and print, per spawn and per correction:

```
overlay: spawn id=3 monitor=\.\DISPLAY2 scale=1.50 physical=(1920,0,2560,1440) chip_physical=(330,54)
overlay: spawn id=3 builder_logical=(1280.0,0.0,220.0,36.0) viewport_ppf=1.50
overlay: viewport id=3 observed_physical=(1920,0,330,54) drift=(0,0)      // outer_rect * native_ppp vs requested
overlay: viewport id=3 ppp 1.00 -> 1.50: re-apply OuterPosition/InnerSize  // correction fired
```

`observed_physical` comes from `raw.viewports[vp].outer_rect * native_pixels_per_point` (`egui-winit
lib.rs:1442-1449`, :1496) — it is the ground truth, so the drift line separates 🅱 (position/size math)
from 🅲 (nothing composited): a chip with drift=(0,0) but no visible pixels is a presentation bug.
Also extend the existing toast-debug sink line (`crates/toast/examples/toast_debug.rs:39-47`) with monitor
index and scale. In virshot, log the enumeration once at INFO (`src/gui.rs` call site): monitor name,
position, size, scale_factor per monitor (`tracing::info!`). eframe's own window-init failures are
`log::error!` (`glow_integration.rs:1363`) and are invisible today — `tracing-log 0.2.0` is already in
`Cargo.lock` as a `tracing-subscriber` default dependency, so `tracing_subscriber::fmt()…with_tracing_logs()`
in `src/logging.rs:170` surfaces them (and `log::debug!("Creating a window for viewport …")`,
`glow_integration.rs:1385`) with **no new crate**, only a subscriber feature/method change. Zero-cost when
the flag is off; 🅲 diagnostics (ghost vs black) still need the Windows run.

### 6. Linux impact and unit tests

- X11/Xvfb: `MonitorHandle::scale_factor()` is 1.0 (`REG/winit-0.30.13/src/platform_impl/linux/x11/monitor.rs:183`)
  ⇒ physical == logical, conversion is a no-op; ui-verify's host flow and the single-monitor dev host behave
  exactly as today. Wayland may report a fractional output scale (`linux/wayland/output.rs:87`) — the same
  rule plus the observed-ppp correction handles it.
- Tests to update (pure functions, no window needed): rename `toast_debug.rs` tests (:300-337, currently
  `…_divides_every_dimension`) to assert PHYSICAL rects preserved + per-monitor scale attached;
  `monitor_rects_non_positive_scale_falls_back_to_one_like_virshot` becomes "scale ≤ 0 ⇒ 1.0".
  `src/gui.rs::monitor_rects_logical` → `monitor_rects_physical` with the same primary-first swap and the same
  skip/WARN behaviour (`src/gui.rs:1117-1136`); toast placement/service tests keep `scale = 1.0`.

### 7. Mixed-DPI caveat status

The Stage 4 "Follow-up caveat (NOT the root cause)" note (this file, :528-533) is **RESOLVED** by this
conversion: per-monitor `scale_factor()` at placement plus the observed-ppp correction makes physical
geometry exact on every monitor. Remaining nuance, documented not open: a DPI transition still preserves
the *logical* size (`event_loop.rs:2274-2281`), so without the correction command a chip would resize by
`s_new/s_old`; the correction re-applies the physical rect whenever the viewport's ppp changes. 🅲
(Windows transparent child presentation: `glow_integration.rs:206-218` skips `glutin_winit::finalize_window`
for transparent windows, so no alpha pixel format is selected while a `gl_surface` is still built from
`window.inner_size()` (:1396-1420); winit's `WindowFlags::TRANSPARENT` maps to NO window style at all —
`windows/window.rs:121-131`, :1325 and `window_state.rs:256-310`, and `WS_EX_NOREDIRECTIONBITMAP` only comes
from the platform-specific `no_redirection_bitmap` (`window_state.rs:280`), which egui-winit never sets
(`egui-winit lib.rs:2289` only calls `with_transparent`) ⇒ alpha blending on Windows children is not
guaranteed by this stack). Separate follow-up, needs the debug lines above to tell "never painted" from
"painted without alpha".

### 8. Windows verification checklist (after implementing §4 + §5)

1. Build: `./cargo.sh build --release --target x86_64-pc-windows-gnu`; run `toast_debug` with debug on
   (and virshot with `OVERLAY_DEBUG`-equivalent flag + `with_tracing_logs()`), on the user's mixed-DPI rig.
2. Debug lines per chip: monitor name (`\.\DISPLAY2`), physical position/size, `scale_factor`,
   `builder_logical`, `viewport_ppf`, `observed_physical`, `drift`. Expect `drift=(0,0)` on EVERY monitor;
   a non-zero `drift` is a 🅱 residual, a zero `drift` with an invisible chip is 🅲.
3. Corner anchors: chip bottom-right on each monitor, inset 30 px / 60 px **physical** — measured from the
   screenshot in physical pixels, not logical (the 60 px inset must clear a ~48 px taskbar on the DPI-scaled
   monitor too).
4. Chip aspect on every monitor ≈ `chip_width/CHIP_HEIGHT` after the observed scale is applied; no ghost
   placeholder cards, no black opaque bar (🅲 check: chip pixels blend, background shows through).
5. Mixed-DPI transitions: drag nothing (chips are not interactive), but change the system scale of the
   secondary monitor mid-run ⇒ expect a `ppp 1.00 -> 1.50: re-apply` line and the chip keeps its physical
   rect; a chip that resized or moved is the `WM_DPICHANGED` path (`event_loop.rs:2274-2281`) not re-corrected.
6. Root window stays correct (regression: `toast debug` panel and virshot settings window unchanged in
   size/position on the primary monitor); taskbar entries for chips stay absent (Stage 7 E).
7. Linux gate: `./scripts/check.sh` (unit tests incl. the rewritten `monitor_rects_*`) and
   `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`; ui-verify host flow on Xvfb (scale 1.0) must
   show chips in the same corners as before — the conversion is a no-op there.

## Stage 8 research (G) — ONE shared monitor enumeration (`crates/overlay/src/monitors.rs`)

Question: which winit 0.30.13 API gives the monitor list + the PRIMARY flag (no unsafe), what order
Windows enumerates in, how much of `crates/toast/examples/toast_debug.rs` and `src/gui.rs` is duplicate,
and whether `crates/overlay/src/monitors.rs` is feasible on Linux + `x86_64-pc-windows-gnu`. Research-only:
no code changed. Registry root shorthand: `REG = .cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f`.

### 1. winit 0.30.13 monitor API — the public surface (verified in vendored source)

- `REG/winit-0.30.13/src/monitor.rs:102-167` — `pub struct MonitorHandle` wrapping the platform handle.
  Derives `Debug, Clone, PartialEq, Eq, PartialOrd, Ord` (:102) — **no `Hash`**, and **no `is_primary()`
  and no primary field on the public handle**. Methods: `name() -> Option<String>` (:112, "None if the
  monitor doesn't exist anymore"), `size() -> PhysicalSize<u32>` (:118), `position() -> PhysicalPosition<i32>`
  (:125), `refresh_rate_millihertz() -> Option<u32>` (:137), `scale_factor() -> f64` (:154, platform notes
  :146-150: X11 overridable via `WINIT_X11_SCALE_FACTOR`, Wayland may differ from `Window::scale_factor`,
  Android always 1.0), `video_modes()` (:164). Coordinates and sizes are PHYSICAL pixels (matches Stage 7 F).
- Primary APIs exist and are SAFE public wrappers — the earlier "winit has no primary API" note (this
  file, Stage 7 F / `src/gui.rs:1139-1140`) means only "no `MonitorHandle::is_primary()`":
  - `REG/winit-0.30.13/src/window.rs:1582-1622` ("Monitor info functions"): `current_monitor()` (:1587,
    "the monitor on which the window currently resides"), `available_monitors() -> impl Iterator` (:1599),
    **`primary_monitor() -> Option<MonitorHandle>` (:1618)** — doc :1606-1616 "Returns the primary monitor
    of the system… **Wayland / Web:** Always returns `None`". All three go through
    `Window::maybe_wait_on_main` (:1589, :1601, :1620).
  - `REG/winit-0.30.13/src/event_loop.rs:396-417`: `ActiveEventLoop::available_monitors()` (:398) and
    **`ActiveEventLoop::primary_monitor()` (:413)** with the same Wayland/Web `None` note (:409-411). These
    call the platform impl directly — no main-thread wait — so they are the thread-backend entry point.
  - Rendered docs agree: `./scripts/cratedoc.sh winit::window::Window` shows `current_monitor` (:727),
    `available_monitors` (:733), `primary_monitor` (:738). **Tooling gap:** method-level item lookups
    (`./scripts/cratedoc.sh winit::window::Window::primary_monitor`, `…ActiveEventLoop::primary_monitor`,
    `eframe::CreationContext::winit_window`) report "item not found; candidates:" — the script renders
    struct pages; use the struct page + `rg` or the vendored source instead.
- There is **no `Window::cursor_position()`** in 0.30.13 — only `set_cursor_position(&self, P) ->
  Result<(), ExternalError>` (`window.rs:1457`, docs :634). A "which monitor is the cursor on" query must
  be containment arithmetic over `Event::CursorMoved` (thread backend) or egui pointer positions (host
  backend), not a winit getter.

### 2. Windows enumeration order — index 0 is NOT a contract for "primary"

- winit's Windows enumeration: `REG/winit-0.30.13/src/platform_impl/windows/monitor.rs:100-111` —
  `pub fn available_monitors() -> VecDeque<MonitorHandle>` calls
  `EnumDisplayMonitors(0, ptr::null(), Some(monitor_enum_proc), &mut monitors as *mut _ as LPARAM)`;
  the callback `monitor_enum_proc` (:89-98) does `(*monitors).push_back(MonitorHandle::new(hmonitor))`.
  **winit sorts nothing** — the order is exactly the callback delivery order.
- MSDN (`https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaymonitors`):
  enumerates monitors "that intersect a region formed by the intersection of a specified clipping
  rectangle and the visible region of a device context"; "calls an application-defined `MonitorEnumProc`
  callback function **once for each monitor that is enumerated**"; with `hdc = NULL` and `lprcClip = NULL`
  the table says "Enumerates all display monitors". The page also notes it includes "invisible
  pseudo-monitors associated with the mirroring drivers" (the reason gui.rs skips zero-size entries) and
  that `GetSystemMetrics(SM_CMONITORS)` "counts only the display monitors". **No ordering guarantee is
  documented** — so primary-first must be produced by us, never assumed from enumeration order.
- Windows primary: `windows/monitor.rs:113-117` — `MonitorFromPoint(POINT{x:0,y:0}, MONITOR_DEFAULTTOPRIMARY)`
  → `MonitorHandle::new(hmonitor)`. MSDN (`.../nf-winuser-monitorfrompoint`): point is in **virtual-screen
  coordinates**; `MONITOR_DEFAULTTOPRIMARY` = "Returns a handle to the primary display monitor".
- Windows `current_monitor`: `windows/monitor.rs:119-122` — `MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST)`,
  i.e. the monitor nearest the *window*, not the OS primary.
- Windows geometry per handle: `get_monitor_info` (:135-146) fills `MONITORINFOEXW` via `GetMonitorInfoW`;
  `size()` (:170-176) and `position()` (:196-203) read **`rcMonitor`** (the full monitor rect, not `rcWork`
  ⇒ taskbar region is inside the rect, per Stage 7 F §3), physical pixels, `position()` falls back to
  `(0,0)` when `GetMonitorInfoW` fails (:202); `name()` (:154-157) decodes `szDevice` (`\\.\DISPLAY1`) but
  internally `.unwrap()`s `get_monitor_info` (:155) — see §10; `scale_factor()` (:206-208) =
  `GetDpiForMonitor(MDT_EFFECTIVE_DPI)/96` (Stage 7 F §2). `rcMonitor.top`/`left` can be **negative** for a
  monitor placed above/left of the primary, since the virtual-screen origin is the primary's top-left.
- Handle equality caveat: `pub struct MonitorHandle(HMONITOR)` derives `Eq/PartialEq` on the raw handle
  (:79-80, `unsafe impl Send` :87). The primary handle comes from `MonitorFromPoint`, the list handles from
  `EnumDisplayMonitors`; MSDN documents neither handle identity across calls nor stability after a display
  settings change ⇒ flag computation must tolerate a non-matching handle (§3 fallback).

### 3. Primary detection without unsafe — ranked

1. **winit `primary_monitor()` + equality flag (chosen).** `Some(&monitor) == primary.as_ref()` over
   `available_monitors()` — already what `src/gui.rs:1161, :1174` does. Safe (no FFI in our code; winit's
   own unsafe is third-party and outside the repo rule). Exact on Windows (`MonitorFromPoint`), X11
   (RandR), macOS (`mainScreen`), always `None` on Wayland/Web. `MonitorHandle: PartialEq` is enough; no
   `Hash` needed.
2. **Fallback when step 1 finds nothing:** the entry whose **position is exactly `(0,0)`** — on Windows the
   primary monitor defines the virtual-screen origin, so primary ⇒ `position == (0,0)`; on X11 RandR gives
   real origins. Prefer equality-with-`(0,0)` over plain containment: containment with inclusive bounds can
   also match a monitor placed to the *left* of primary whose right edge is `x = 0`, and
   `GetMonitorInfoW`-failure yields `(0,0)` (`windows/monitor.rs:202`) ⇒ a stale handle could masquerade as
   primary. Last resort: the first enumerated entry (matches eframe's own precedent, §4).
3. **`current_monitor()` — wrong semantics.** It answers "which monitor holds *this* window"
   (`window.rs:1583-1587`), and the settings window may sit on a secondary monitor; egui-winit uses it only
   to fill `ViewportInfo::monitor_size` for the current viewport (`REG/egui-winit-0.37.0/src/lib.rs:1487`).
   Not a substitute for OS primary.
4. **egui/eframe — no public monitor-list API.** egui exposes only `ViewportInfo::monitor_size` (points,
   root viewport). eframe's own integration uses exactly the shape we want:
   `REG/eframe-0.37.0/src/native/epi_integration.rs:66-70` — `event_loop.primary_monitor().or_else(||
   event_loop.available_monitors().next())` for initial window placement; `largest_monitor_point_size`
   (:97-119) divides sizes by `scale_factor` ⇒ egui point space, unusable for physical placement (Stage 7
   F §2). `egui-winit` calls `available_monitors()` internally only (:2067, :2225).
- **Thread affinity:** `Window::available_monitors()/primary_monitor()` are wrapped in `maybe_wait_on_main`;
  the Windows impl is a pass-through (`REG/winit-0.30.13/src/platform_impl/windows/window.rs:105-108`),
  macOS waits on the main thread. Enumerate on the thread that owns the event loop — eframe's main thread
  for `src/gui.rs`, the overlay loop thread via `ActiveEventLoop` for the thread backend — never from the
  toast timer thread (`crates/toast/src/service.rs` is plain `std`, no winit).

### 4. X11 / Wayland behaviour (Linux dev target)

- **X11** keeps a private primary flag: `REG/winit-0.30.13/src/platform_impl/linux/x11/monitor.rs:59-60`
  (`primary: bool`), constructor param (:106-136); discovery sets it from
  `randr_get_output_primary(root.root)?.reply()?.output` (:242) and `is_primary = crtc.outputs[0] == primary`
  (:257); if RandR names none, the **first** monitor is marked primary (:263-268, comment :266 "Setting this
  here will come in handy if we ever add an `is_primary` method"); `primary_monitor()` (:289-295) =
  `available_monitors().into_iter().find(|m| m.primary).unwrap_or_else(MonitorHandle::dummy)` ⇒ **always
  `Some` on X11** unless the X connection errors. Equality compares the RandR `crtc` id (:71-77), so the
  equality flag is stable; enumeration **drops** crtc entries with `width == 0 || height == 0 ||
  outputs.is_empty()` (:253-255) ⇒ no zero-size entries on X11, and the list is cached (:274-286,
  `DISABLE_MONITOR_LIST_CACHING`) so hotplug needs a fresh process/loop.
- **Wayland** has no primary concept: `REG/winit-0.30.13/src/platform_impl/linux/wayland/output.rs:17-21`
  — `primary_monitor()` returns `None` ("There's no primary monitor on Wayland"). `size()` is the current
  mode else `(0,0)` (:47-58), `position()` is the **logical** position multiplied by the output
  `scale_factor` (:60-72), `scale_factor()` is the **integer** output scale (:78-82), equality by
  `native_identifier()` (:101-105). Dispatch is `x11_or_wayland!`
  (`REG/winit-0.30.13/src/platform_impl/linux/mod.rs:222-257`), `scale_factor() -> f64` = cast (:247-250).
  Window geometry is unavailable on Wayland: `inner_position()`/`outer_position()` return
  `NotSupportedError` (`window.rs:678`, :701) ⇒ the "window's own rect" fallback must use `inner_size()`,
  not a window position.
- X11 scale source: `REG/winit-0.30.13/src/platform_impl/linux/x11/util/randr.rs:56-145` —
  `WINIT_X11_SCALE_FACTOR` (`randr` or a float; the old `WINIT_HIDPI_FACTOR` is deprecated; an **invalid
  value panics**), else `Xft.dpi`, else millimetre-based `calc_dpi_factor`.

### 5. Duplication map — `crates/toast/examples/toast_debug.rs` ↔ `src/gui.rs`

| Concern | toast_debug.rs | src/gui.rs | verdict |
| --- | --- | --- | --- |
| handle tuple → `MonitorRect` (physical + `MonitorRect::new`) | `monitor_rects` :116-126 | `monitor_rects_physical` :1106-1136 | **duplicate**, gui adds zero-size skip + WARN (:1112-1115) and primary-first `swap(0, index)` (:1130-1134) |
| enumeration from winit (`position/size/scale_factor`) | `host_monitors` :132-144 | `monitor_metrics` :1159-1180 (+ `is_primary` :1174, `name` :1175) | **duplicate** shape; only gui adds the primary flag |
| "no monitors" fallback = window inner size × window scale | :148-154 | :1266-1276 | **duplicate arithmetic** |
| egui-points fallback (`monitor_size` → `inner_rect` → 800×600, × ppp) | :156-176 | :1229-1248 | **duplicate arithmetic**, identical numbers |
| debug line `#N name pos= size= scale= [primary]` | — (only per-chip `monitor=<index> scale= rect=` in `LoggingSink` :62-69) | `monitor_metrics_line` :1185-1203 + `monitor_debug_line` :1211-1219 | **gui-only formatting**; toast_debug needs the same line for its own evidence |
| chip rect → monitor index (`monitor_of`) | :41-52 | — | **unique to toast_debug** (12-line containment, not duplicated) |
| `MonitorInfo` plain-fields struct | — | :1145-1153 | **unique**, exists only to keep both halves unit-testable without winit types (:1138-1144: "virshot has no winit dependency") |
| primary-first contract doc | `MonitorSel::Index(0)` :243-246 | crate doc :81-88, `monitor_sel` `src/capture.rs:195-200` | same semantics, two places |
| Tests to move with the code | `#[cfg(test)]` :352-417 | `src/gui.tests.rs:368-445` | both test the duplicated halves |
| `crates/overlay/examples/overlay_demo.rs` | synthetic single monitor (:13 "the demo has no display enumeration yet", `scale: 1.0` :128) | — | nothing to dedupe; may adopt the module later |

### 6. Feasibility of `crates/overlay/src/monitors.rs` (Q4)

- **winit is a direct, non-optional dependency of overlay** (`crates/overlay/Cargo.toml:10` `winit = "0.30"`),
  so `winit::window::Window` / `winit::event_loop::ActiveEventLoop` are usable in lib code on every platform;
  `Cargo.lock:4618-4621` pins **0.30.13** (already approved in task 2026-11-04_overlay-toast ⇒ **no new
  crates.io deps**). Linux compiles the x11/wayland impls (eframe features `x11`/`wayland`, unix-only per
  `crates/overlay/Cargo.toml:22-25`), windows-gnu compiles the windows impl — overlay already builds winit
  there (Stage 7 gates ran clippy + `test --no-run` for `x86_64-pc-windows-gnu`).
- **`eframe::CreationContext` must stay OUT of the module**: eframe is a **dev-dependency only**
  (`crates/overlay/Cargo.toml:24-25`). The accessor the callers use is
  `REG/eframe-0.37.0/src/epi.rs:138-145` — `#[cfg(not(target_arch = "wasm32"))] pub fn
  winit_window(&self) -> Option<&std::sync::Arc<winit::window::Window>>` (`None` for headless; docs struct
  page :45; the active-viewport twin at :819). So `monitors.rs` takes `&winit::window::Window` (host callers
  pass `cc.winit_window()`) and optionally `&winit::event_loop::ActiveEventLoop` (thread backend).
- **Consumers add no dependencies:** root `virshot` has the `overlay` path dep and no winit dep (that is the
  documented reason for `MonitorInfo`, `src/gui.rs:1141-1144`) — calling `overlay::monitors::…(&window)` on
  the `&Arc<winit::window::Window>` returned by `cc.winit_window()` needs no winit import in `src/gui.rs`;
  `crates/toast` (`Cargo.toml:10-12`: overlay/egui/thiserror, dev eframe+image) likewise reaches it through
  `overlay`, and its example already uses `cc.winit_window()`.
- **Module conventions:** `crates/overlay/src/lib.rs` — `#![deny(unsafe_code)]` (:42), private modules
  `content/error/gl_boot/host/placement/service` (:44-49) with selective re-exports (:51-56). A new
  consumer-facing module needs **`pub mod monitors;`** (not a private `mod`), one module = one file, with
  its own `#[cfg(test)] mod tests` at the end of the file (the `placement.rs` pattern). Zero `unsafe`: every
  call is winit's safe wrapper; `gl_boot.rs` stays the only `#![allow(unsafe_code)]` site.
- **Reuse existing types:** `crates/overlay/src/placement.rs` `pub struct Rect { x, y, w, h }` (:16-25) +
  `Rect::new` (:29) and `pub struct MonitorRect { pub rect: Rect, pub scale: f32 }` (:44-50) whose
  `MonitorRect::new` normalises a non-finite/non-positive scale to `1.0` (:54-63). No new rect type; the
  module returns `Vec<MonitorRect>` built through `MonitorRect::new`.

### 7. Recommended shape (the one implementation)

`crates/overlay/src/monitors.rs` (`pub mod monitors;`), responsibility = "enumerate monitors + primary flag":

```rust
pub struct MonitorEntry {           // plain fields ⇒ unit-testable, no winit types in tests
    pub x: i32, pub y: i32, pub width: u32, pub height: u32,
    pub scale: f64, pub is_primary: bool, pub name: Option<String>,
}

fn entries(handles: impl IntoIterator<Item = MonitorHandle>,
           primary: Option<&MonitorHandle>) -> Vec<MonitorEntry>;   // private, shared by both entry points
pub fn from_winit_window(window: &winit::window::Window) -> Vec<MonitorEntry>;   // host callers: cc.winit_window()
pub fn from_active_event_loop(loop_: &winit::event_loop::ActiveEventLoop) -> Vec<MonitorEntry>; // thread backend
pub fn rects(entries: &[MonitorEntry]) -> Vec<MonitorRect>;   // skip zero-size, attach scale, PRIMARY FIRST
pub fn debug_line(entries: Option<&[MonitorEntry]>) -> String; // "#N … primary | #N …" + two sentinel messages
pub fn fallback_rect(window: &winit::window::Window) -> MonitorRect;            // inner_size × window scale, origin (0,0)
pub fn fallback_rect_from_points(points: [f64; 2], ppp: f64) -> MonitorRect;    // egui points → physical px, scale 1.0
```

- primary flag: `primary` equality (§3 step 1), else `x == 0 && y == 0` (step 2), else `false`.
- **primary-first ordering lives in `rects()`** (gui's `swap(0, index)` semantics, `src/gui.rs:1130-1134`),
  so toast `MonitorSel::Index(0)` keeps meaning "primary" for BOTH consumers and the debug `#0` line always
  names the primary — the ordering rule is stated once, in the module doc.
- `Option<&[MonitorEntry]>` in `debug_line` keeps the two sentinels (`None` → "no winit window (toast uses
  the root viewport's rect)", empty → "winit reported no monitors") — keep the exact wording the Windows
  rig checklist (§ Stage 7 F :891-910) expects.
- `rects()` does not log; the caller compares `entries.len() != rects.len()` to emit the zero-size WARN
  (library functions must not swallow or print).
- containment (`monitor_of`) stays in `toast_debug.rs` — it is example-local chip evidence, not enumeration,
  and placement arithmetic already belongs to `placement.rs` (SRP).

### 8. What each consumer keeps / deletes after the refactor

- `src/gui.rs`: delete `monitor_rects_physical` (:1106-1136), `MonitorInfo` (:1145-1153),
  `monitor_metrics` (:1159-1180), `monitor_metrics_line` (:1185-1203), `monitor_debug_line` (:1211-1219);
  `host_monitor_rects` (:1227-1277) becomes a thin adapter — `cc.winit_window()` →
  `overlay::monitors::from_winit_window` → `rects()` when non-empty → `fallback_rect(window)` /
  `fallback_rect_from_points(...)` otherwise — keeping its two WARN logs (:1229, :1266); the startup line
  (:1326) calls `overlay::monitors::debug_line(...)`. `src/gui.tests.rs:368-445` move to
  `crates/overlay/src/monitors.rs`'s tests (pure tuples + wording); gui keeps no monitor-specific test.
- `crates/toast/examples/toast_debug.rs`: delete `monitor_rects` (:116-126), the enumeration block in
  `host_monitors` (:132-144) and both fallback blocks (:148-154, :156-176); `host_monitors` becomes
  `overlay::monitors::rects(&overlay::monitors::from_winit_window(window))` + fallbacks, plus the debug
  `debug_line` in the panel; keep `monitor_of` (:41-52) and the spawn log (:62-69); with primary-first
  rects, the unchecked "all monitors" checkbox (:243-246) keeps `MonitorSel::Index(0)` = primary, no new
  index math. Its `monitor_rects_*` / `monitor_of_*` tests (:352-417) move with the shared code except
  `monitor_of_*`.
- Untouched: `crates/toast/src/placement.rs:22-50`, `crates/toast/src/service.rs`, `src/capture.rs:195-200`
  (Index(0) = primary still holds), `crates/overlay/src/host.rs` monitor storage (:130, :167-180),
  `crates/overlay/examples/overlay_demo.rs` synthetic monitor (:13, :128).

### 9. Constraints to record for Stage 8 implementation

- **No new crates.io dependencies** — winit 0.30.13 is already locked (`Cargo.lock:4618-4621`) and approved;
  the module imports only `winit::window::Window`, `winit::event_loop::ActiveEventLoop`,
  `winit::monitor::MonitorHandle` and `overlay::placement::{Rect, MonitorRect}`.
- **edition 2024**; **one module = one file** (`crates/overlay/src/monitors.rs`, `pub mod monitors;` in
  `lib.rs`); tests live in that file's `#[cfg(test)] mod tests`, named
  `<unit>_<scenario>_<expectation>`.
- **Safety:** overlay keeps `#![deny(unsafe_code)]` (`crates/overlay/src/lib.rs:42`); `monitors.rs` adds
  zero `unsafe`; `gl_boot.rs` remains the single module-scoped `#![allow(unsafe_code)]` site. No `unwrap`
  outside tests/startup — every fallback returns a value.
- **Cargo only via `./cargo.sh`** (`CARGO_HOME=./.cache/cargo`); scratch in `./.tmp/`; gates
  `./scripts/check.sh` per batch, `./scripts/check_full.sh` at stage boundary, plus the Windows compile
  gates `./cargo.sh clippy --workspace --all-targets --target x86_64-pc-windows-gnu` and
  `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` (executed Windows tests need a Windows host).
- **Keep the pure half testable**: enumeration-touching code stays a thin wrapper over tuple/entry-based
  helpers so `rects()`, `debug_line()`, primary selection are unit-tested without a real display
  (Xvfb gives one synthetic monitor only).
- **One winit `EventLoop` per process** (repo thread-model note): enumerate on the owning loop thread; the
  module must never be called from the toast timer thread.

### 10. Blockers / open risks (record, do not silently fix)

1. **Wayland has no primary** (`wayland/output.rs:17-21`) ⇒ the flag comes from the `(0,0)` heuristic; on a
   single-output compositor that is `(0,0)` and harmless, on multi-output compositors nothing may be
   flagged ⇒ `Index(0)` = first enumerated output. Document in the module doc; Linux is a dev target, the
   product is Windows.
2. **Windows handle identity is not guaranteed** (§2): if `MonitorFromPoint`'s `HMONITOR` does not compare
   equal to an enumerated handle (settings change, mirroring pseudo-monitors), the equality flag misses ⇒
   the `(0,0)` fallback covers it. Also "invisible pseudo-monitors" from mirroring drivers are enumerated,
   which is why the zero-size skip stays.
3. **`MonitorHandle::name()` can panic on Windows** — `windows/monitor.rs:154-157` `.unwrap()`s
   `get_monitor_info` (:155) if `GetMonitorInfoW` fails. gui.rs already calls it (:1175) so the risk is
   unchanged, but the shared module should call `name()` in the same pass and only when the debug line is
   wanted. Same class of risk on X11: an invalid `WINIT_X11_SCALE_FACTOR` panics inside winit
   (`x11/util/randr.rs:56-145`).
4. **No cursor getter** (§1) — cursor→monitor must be containment arithmetic, not a winit call.
5. **X11 caches the monitor list** (`x11/monitor.rs:274-286`) ⇒ hotplug/re-DPI needs a fresh enumeration
   (new process or `DISABLE_MONITOR_LIST_CACHING`); Windows enumerates per call, so live re-enumeration is
   cheap there.
