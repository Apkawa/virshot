# 2026-11-04_overlay-toast: implementation plan (overlay + toast)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan
- [x] Capture draft.md (raw request + user clarifications)
- [x] Research: workspace state, locked egui/eframe/winit stack, content-protection/click-through
  APIs, existing toast machinery (src/overlay.rs, src/gui.rs), capture emission point, gates
- [x] spec.md v1 + review (image API = bytes + load(path) helper; tests = pure-logic only)
- [x] plan.md

**Criterion:** spec.md is v1 with `Created`/`Changes`, no open questions; plan accepted by user.
**Commit:** `docs(spec): overlay/toast task draft, research, spec v1, plan`

## Stage 1 — Workspace + crate skeletons
- [x] Root `Cargo.toml`: add `[workspace] members = [".", "crates/overlay", "crates/toast"]`,
  `exclude = ["crates/gamepad", "hidpad-poc"]` (cargo auto-adds gamepad as member via path-dep;
  hidpad-poc gained an empty `[workspace]` table to keep its standalone build working)
- [x] `crates/overlay/Cargo.toml`: lib `overlay`, edition 2024, deps (locked versions only):
  `winit = "0.30"`, `glutin = "0.32"`, `glutin-winit = "0.5"`, `glow = "0.17"`, `egui = "0.37"`,
  `egui-winit = "0.37"`, `egui_glow = "0.37"`, `image = "0.25"` (default-features=false,
  png/jpeg/webp), `thiserror = "2"`; dev-dep `eframe = "0.37"` (examples); crate root
  `#![forbid(unsafe_code)]`; minimal `src/lib.rs`
- [x] `crates/toast/Cargo.toml`: lib `toast`, deps `overlay = { path = "../overlay" }`,
  `egui = "0.37"`, `thiserror = "2"`; dev-deps `eframe = "0.37"`, `image = "0.25"`;
  `#![forbid(unsafe_code)]`; minimal `src/lib.rs`
- [x] `./scripts/check.sh` green (workspace-wide); `./cargo.sh test --no-run
  --target x86_64-pc-windows-gnu` green

**Criterion:** workspace resolves; both crates compile on Linux and windows-gnu; root virshot
build unchanged (no toast dep yet); dependency list = locked versions only (user-approved).
**Commit:** `feat(workspace): add crates/overlay and crates/toast as workspace members`

## Stage 2 — crates/overlay: low-level overlay library
- [x] Red tests for pure logic (`crates/overlay/src/placement.rs`): monitor rect + corner +
  insets (30px vertical / 60px horizontal / bottom corners +48px) → window position/size;
  click-through/content-protected flags passthrough
- [x] Implement `placement` + tests green
- [x] Overlay thread core: single winit event-loop thread (winit 0.30 allows exactly ONE
  EventLoop per process — `EventLoopBuilder::build` guards with a global flag; N windows are
  multiplexed via `EventLoopProxy`), glutin/glutin-winit surface init, egui+glow paint pass;
  window attrs: transparent, decorations(false), always_on_top, `content_protected`, position/size;
  `spawn_window` blocks on a one-shot handshake until the window is paintable
- [x] Non-blocking facade: `OverlayHandle` (Send+Sync) over `Arc<Mutex<Sender<OverlayCommand>>>`
  (unbounded `std::sync::mpsc`); `redraw/hide/quit_all` return immediately; click-through →
  `window.set_cursor_hittest(false)`; drop handle → `quit_all`
- [x] Content primitives: text, image bytes → `egui::TextureHandle` (decode via `image`,
  cached), painter callback `Fn(&mut egui::Ui) + Send`; fade alpha as paint parameter
  (timers live in toast, not overlay)
- [x] Refactor; `./scripts/check.sh` green (clippy -D warnings, workspace + windows-gnu
  compile-only) — deviation accepted: `#![deny(unsafe_code)]` + module-scoped
  `#![allow(unsafe_code)]` confined to `gl_boot.rs` (glutin context/surface creation is
  unsafe-only; mirrors the `crates/gamepad` shim precedent)
- [x] `crates/overlay/examples/overlay_demo.rs`: console app (chip → fade → painter → PNG
  chip → corner cycle → hide → quit_all). Linux run: partial only — one run reached window
  creation then stalled in winit's X11 loop construction under Xvfb (pure-XCB probe works,
  stall is inside winit's Xlib/xkbcommon path; sandbox limitation, logs in `.tmp/`);
  verified compile-only + windows-gnu build

**Criterion:** `./cargo.sh test -p overlay` green; demo compiles and (Linux) reaches window
creation — full render verification deferred to Windows by the user (sandbox X11/xvfb stall
inside winit loop construction is an environment limitation, recorded in `.tmp/stage2-notes.md`);
facade non-blocking; no unsafe outside `gl_boot.rs` (scoped allow, documented).
**Commit:** `feat(overlay): winit event-loop overlay thread with non-blocking mpsc facade`

## Stage 2b — overlay host-loop backend (forced: winit 0.30 = one EventLoop per process)
- [x] Red tests: `OverlayHost` command semantics — drain (latest-wins per id), redraw on
  unknown id, hide removes entry, update from host thread only (17 tests)
- [x] Implement `crates/overlay/src/host.rs`: `OverlayHost` over `egui::Context` —
  `update(ctx)` drains mpsc commands and paints toast windows as egui multi-viewports
  (`ViewportBuilder`: transparent, decorations(false), `WindowLevel::AlwaysOnTop`,
  mouse_passthrough; `ViewportCommand::ContentProtected` one-shot after window exists;
  deferred viewport + first-paint-under-hiding sequencing ported from virshot
  `src/gui.rs:531,:1250,:1284,:1292`); image decode + textures via `ctx.texture_manager`
  on the host thread; reuse `placement`/`content` painting helpers
- [x] Windows any_thread fix (user Windows run of overlay_demo.exe panicked at winit
  windows/event_loop.rs:190): `#[cfg(windows)] EventLoopBuilderExtWindows::with_any_thread(true)`
  in service.rs, symmetric with the X11 gating; windows-gnu example rebuilt for re-run
- [x] Refactor; `./cargo.sh test -p overlay` (30 pass), `./cargo.sh clippy --workspace -- -D warnings`,
  windows-gnu compile-only green (incl. `check --workspace --all-targets`)

**Criterion:** `OverlayHandle` (thread backend) and `OverlayHost` (host backend) share command
semantics; OverlayHost updated from host main thread only; gate green both targets.
Note for Stage 3/4: host backend takes **egui logical points** (egui scales viewport
position/inner_size by pixels_per_point); thread backend takes **physical pixels** — virshot
must convert xcap physical → logical via `ctx.pixels_per_point`.
**Commit:** `feat(overlay): host-loop backend driven by egui::Context multi-viewports`

## Stage 3 — crates/toast: notification library on overlay
- [x] Red tests for pure rules (ported from `src/overlay.rs` + `src/gui.rs` helpers): phase
  machine (`Hold → FadeOut → Gone`, FADE_MS 250, ERROR_HOLD 3000, duration_ms, 0 = sticky),
  corner placement via overlay placement, palette for default/success/danger/warning,
  monitor choice (`Index(n) | All`), image decode from bytes (PNG) + `load(path)` helper
- [x] Implement rules green (TDD — 40 tests)
- [x] Timer thread — plain `std::thread` (no winit, second EventLoop impossible): loop of
  `recv_timeout(until next phase transition)`; sticky toasts block on `recv()` without ticks;
  drives a **sink** (`trait ToastSink`: `spawn_window`/`redraw`/`hide`) implemented directly
  for `overlay::OverlayHandle` (standalone) and `overlay::OverlayHostHandle` (host);
  fade pushed as 25ms redraw ticks (overlay has no timers); optional `wake: Option<egui::Context>`
  hook calls `request_repaint()` after each forwarded action
- [x] API: `Toast { monitor: MonitorSel, text, image: Option<Vec<u8>>, color, corner, duration_ms }`,
  `ToastHandle { show, dismiss, update }` — non-blocking via mpsc facade; `load(path) -> bytes`
  (size-gated, no decode); latest-wins replacement hides previous chips, restarts at full alpha;
  monitors injected at spawn (deterministic; empty monitors → logged no-op)
- [x] Refactor; `./cargo.sh test -p toast` (40 pass), `./cargo.sh clippy --workspace -- -D warnings`,
  check.sh, windows-gnu compile-only green
- [x] `crates/toast/examples/toast_debug.rs`: debug window (eframe — owns the process loop;
  `OverlayHost` painted in its logic-pass) — 4 corner buttons, color picker, duration field
  (0 = sticky), monitor picker, image toggle (runtime-encoded PNG), Show / Hide
- [x] Run debug example under xvfb (single `xvfb-run` session, `env -u WAYLAND_DISPLAY`):
  chip rendered bottom-right at insets (rect 1670,984,220,36), fade ticks to alpha 0.09, hide
  recorded; screenshots `.tmp/toast-debug-shots/` (chip.png = green "Saved: shot-2.png");
  notes `.tmp/toast-stage3-notes.md`

**Criterion:** `./cargo.sh test -p toast` green (phase/placement/palette/monitor/image tests);
debug example shows toasts in all 4 corners under xvfb, auto-hide fires, sticky survives until
dismiss, dismiss hides immediately.
**Deviations accepted:** `dismiss()` without id (latest-wins ⇒ one current toast); `update` ≡
`show`; monitors snapshot injected at spawn (Stage 4 must feed logical-point rects — xcap
physical ÷ `ctx.pixels_per_point()`); palette additions success/warning (virshot painted one
neutral chip); `IMAGE_CHIP_SIZE=160`, `MAX_IMAGE_BYTES=4MiB`.
**Commit:** `feat(toast): toast rules on std timer thread with sink-driven overlay backends`

## Stage 4 — Integrate toast into virshot
- [x] Root `Cargo.toml`: path deps `overlay` + `toast` (all platforms; no crates.io entries)
- [x] `OverlayHost` + `ToastService` created in eframe `create` closure (logical rects:
  `xcap::Monitor::all() ÷ ctx.pixels_per_point()`, primary first), owned by `SettingsApp`
  (service declared before host so Drop Quit+join runs while host alive); handle delivered to
  trigger threads via `capture::ToastHandoff` = `Arc<Mutex<Option<ToastHandle>>>` (latest-only) —
  deviation: trigger threads start before gui::run, one-shot mpsc couldn't serve them
- [x] `src/capture.rs`: `run_capture(command, now, toast)` — handle is a separate parameter
  (AGENTS.md struct-design rule); `show` for Success{file name}/Danger{verbatim message};
  `ToastMailbox` plumbing removed from `main.rs`; headless path `toast == None` → path printed,
  notification skipped with log, no host/service created
- [x] `src/gui.rs`: logic pass calls `overlay_host.update(ctx)` each frame (works on visible and
  hidden `run_logic` pass); deleted all toast machinery — deferred viewports, `TOAST_*`,
  `toast_window_size/toast_position/toast_positions/toast_placements/toast_show_commands/
  update_toast`, fade logic; SettingsApp keeps the settings window; tray/hotkeys/gamepad intact
- [x] Removed `src/overlay.rs` + `src/overlay.tests.rs` (421 lines); `mod overlay` gone from main.rs
- [x] Config gating preserved 1:1: position→Corner, duration_ms→u32 saturating (0=sticky,
  Danger 3000 override), on_all_monitors→`MonitorSel::All`/`Index(0)`, enabled gates SUCCESS only
  (errors always show); config format unchanged
- [x] Tests: gui toast tests deleted (rules live in toast crate — 40 tests); 14 pure toast tests
  added in capture.tests.rs (gapping/text/corner/monitor/duration/handoff); check.sh green
  (249 unit + 9 cli_headless); windows-gnu `test --no-run` green
- [x] `docs/spec.md` §3/§4.3/§6 rewritten: chip = OverlayHost child viewport (content-protected,
  click-through), Toast Timer Thread section (Quit+join on drop), logical monitor rects
- [x] Xvfb resident smoke VERIFIED (no debug hooks): chip 296×36 at (954,928) =
  `1280−296−30`, `1024−36−60` (BottomRight insets), disappears after hold+fade; screenshots
  `.tmp/stage4-smoke/shots/`; `ui-verify.sh all` exit 0

**Criterion met:** gate green on Linux + windows-gnu compile; resident toast verified on Linux
Xvfb; no leftover toast code in src/; config semantics unchanged (errors show when disabled).
**Commit:** `feat(virshot): replace local toast machinery with crates/toast over OverlayHost`

## Stage 5 — Linux verification + docs cleanup
- [x] Run `scripts/ui-verify.sh` (or xvfb) for virshot + toast example; verify dual event loop
  (eframe main + overlay thread) coexists on Linux; record observations (transparency,
  passthrough, Wayland/X11 caveats, capture-exclusion behavior on X11) in `.tmp/` and
  docs/spec.md §4.3
  → **verified by Stage 5**: fresh `./scripts/ui-verify.sh all --keep --timeout 30` at HEAD —
  exit 0 (toast service started, chip child window next to settings, corner shot non-blank,
  hotkey/tray/lock green); re-run stage-4 lifecycle smoke — chip 296×36 at (954,928) =
  BottomRight insets (30/60), gone after hold+fade, headless CLI creates no toast machinery.
  The shipped process is eframe's single winit loop + the plain std toast timer thread (no
  overlay thread in virshot — winit 0.30 allows one EventLoop per process). Wayland/X11
  transparency and passthrough caveats are already in docs/spec.md §4.3 (Stage 4); X11
  capture-exclusion is Windows-only and deferred to Windows acceptance. Observations:
  `.tmp/stage5-notes.md`.
- [x] Update `AGENTS.md`: workspace members (overlay/toast), gamepad stays excluded,
  path-deps list; mark `.dumbspec/backlog.md` toast-extraction item done
  → **verified by Stage 5**: AGENTS.md dependency bullet now names `crates/overlay` +
  `crates/toast` and the locked stack (approved during this task, no new crates.io packages);
  workspace facts corrected — `crates/gamepad` **is** a member (cargo auto-adds path deps,
  `exclude` cannot veto one; it stays standalone-buildable), `hidpad-poc` gained an empty
  `[workspace]`; Safety records the `gl_boot.rs` scoped-`unsafe` exception; one thread-model
  line added; backlog toast-extraction item marked `[x]`. `.dumbspec/current/` left intact
  (archiving needs user confirmation).
- [x] `./scripts/check_full.sh` green (fmt at boundary)
  → **verified by Stage 5**: check.sh (249 unit + 9 cli_headless, clippy `-D warnings`) +
  `cargo fmt` + `cargo fmt --check` exit 0; `./cargo.sh clippy --workspace --all-targets
  -- -D warnings` green after fixing two stale pre-existing test lints (`src/gui.tests.rs`
  `needless_character_iteration`, `src/tray.tests.rs` `chunks_exact_to_as_chunks`);
  `./cargo.sh test --no-run --workspace --target x86_64-pc-windows-gnu` green; Linux release
  binary 14,865,904 B (~14.2 MB) recorded in `.tmp/stage5-notes.md`.

**Criterion:** Linux run notes recorded (incl. dual-loop coexistence); docs/AGENTS reflect the
new crates; full gate green.
**Commit:** `docs: record overlay/toast integration, workspace layout, Linux verification notes`

## Stage 6 — Windows acceptance fixes (user run 2026-11-04, 19:29)
User ran the Windows builds; app itself OK, three toast/overlay defects:
(A) `overlay_demo.exe` — brief flash, then `GL context creation attempt failed` x3
(glutin OS Error -1073283059 = 0xC007000D, FormatMessageW error 317), `spawn_window` fails.
(B) toast chip content invisible on Windows — only chip shadow shows (same in `toast_debug`
and in virshot; one library root cause). Chip window appears as an opaque white rectangle
in the screenshot. "даже на окна не получается списать" — interpretation pending research.
(C) toast does not show on all monitors even with the "all monitors" checkbox (`toast_debug`
shows exactly one monitor entry — enumeration likely primary-only).
(D) `toast_debug` window has a black background, poorly readable (cosmetic).
- [x] Research root causes (append to research.md as found): (A) gl_boot.rs pixel-format/
  config attempts vs glutin Windows display path — no new deps (ANGLE/DLL bundling would be
  a new dep, needs user approval); (B) egui-winit deferred child-viewport transparency +
  painting on Windows (why fill/text not drawn, background opaque white); (C) MonitorSel::All
  path — reproduce on Linux Xvfb with xrandr second screen, check toast_debug monitor
  enumeration (ctx.input viewport().monitor_size = primary only) and sink fan-out.
  → **done**: (A) gl_boot.rs `build(None)` → WGL path never `SetPixelFormat`s the window DC
  (glutin docs "WGL: you must pass a raw_window_handle"; eframe passes it). (B) click-through,
  NOT transparency: `set_cursor_hittest(false)` → `WS_EX_TRANSPARENT|WS_EX_LAYERED`; winit
  0.30.13 never attributes the layered window → composites nothing (white surface + DWM
  shadow); X11 input-shape unaffected. External answer (user, .tmp/overlay-fixes-notes.md)
  verified against vendored 0.37: shadow-flag claim false (macOS-only), egui#8423 present but
  transparency stays a hint, repaint handled by eframe, CS_OWNDC false. (C) fan-out in the
  library is correct (one spawn per rect, N-rect repro on Xvfb = N chips); bug is the example's
  primary-only enumeration. research.md sections A/B/C.
- [x] Fix (C): monitor enumeration + all-monitors fan-out (red test first, Linux xrandr
  repro; example enumeration fix; xcap as dev-dep for the example = reuse of an approved
  crate — flag to user before adding)
  → **done without dev-dep**: toast_debug enumerates `cc.winit_window().available_monitors()`
  ÷ root pixels_per_point (pure `monitor_rects` helper + 5 red→green unit tests); fallback to
  viewport().monitor_size; Xvfb smoke = 1 chip on single screen (correct degenerate), fan-out
  proven by repro. No manifest changes.
- [x] Fix (B)+(D): chip content visible + background readable — fix in overlay/toast/example
  so virshot inherits the fix; verify on Linux Xvfb, rebuild Windows binaries, user re-runs
  → **done**: passthrough gated off on Windows in BOTH backends (host.rs:272-283,
  service.rs:225-234) + one-line demo note; toast_debug light visuals + transparent root +
  alpha-0 clear + opaque light CentralPanel; chip painting untouched. Linux: check.sh +
  ui-verify.sh all green; chip crop mean 33.6/stddev 17.7 (content painted), panel mean 238.5
  (light). Windows re-run pending (chip composites, ContentProtected re-composite).
- [x] Fix (A): overlay_demo GL context on Windows (config attempts/fallback per research);
  rebuild windows-gnu example, user re-runs
  → **done**: gl_boot.rs create_context takes &Window, `build(Some(window_handle))`
  (SetPixelFormat lands on the DC before wglCreateContext; EGL/GLX ignore the handle —
  vendor-verified zero reads); attempt loop unchanged; Linux Xvfb demo smoke 3× PASS
  (chip renders, no GL-failure lines). overlay_demo.exe release rebuilt; Windows re-run
  pending (no "GL context creation attempt failed").
- [x] Gates: `./scripts/check.sh`, `clippy --workspace --all-targets -D warnings`,
  `-p overlay`/`-p toast` tests, `test --no-run --workspace --target x86_64-pc-windows-gnu`,
  release windows-gnu virshot.exe + overlay_demo.exe rebuilt; commit
  → **done**: check_full.sh green (249 unit + 9 E2E + clippy + fmt); clippy --all-targets
  clean linux AND windows-gnu; windows-gnu test --no-run green; ui-verify.sh all exit 0;
  one deterministic test fix included (`pick_foreground` foreign pids → own_pid+1/+2,
  impl untouched). Artifacts: virshot.exe 8 127 488 B (<10MB ✓), overlay_demo.exe 6 278 656 B,
  toast_debug.exe 6 831 104 B. docs/spec.md §4.3/§9 Russian updated (passthrough Windows
  limitation, all-monitors enumeration, mixed-DPI caveat, transparency-hint caveat).

**Criterion:** Windows — overlay_demo renders chips without crashing; chip content (fill+text)
visible in toast_debug and virshot; "all monitors" yields a chip on every monitor; debug UI
readable. Linux — gates green, xrandr repro of (C) green, no regressions.
→ Linux half met; Windows acceptance = user re-run pending (demo.exe, toast_debug.exe,
virshot.exe rebuilt — see Stage 6 verification request to user).
→ **User confirmed 23:37**: toasts render (Stage 6 B fixed) and are excluded from screenshots;
remaining defects (taskbar visibility, monitors 2/3 placement) moved to Stage 7.
**Commit:** `fix(overlay,toast): Windows GL bootstrap, chip rendering, all-monitors fan-out`

## Stage 7 — Windows acceptance fixes 2 (user run 2026-11-04, 23:37)
User confirmed after Stage 6: toasts work, excluded from screenshots. Two new defects:
(E) overlay/toast windows appear in the Windows taskbar (toast debug + 3 chip windows);
must not appear there.
(F) chips on monitors 2/3 are SPAWNED (3 windows exist) but render wrong on those monitors
— white gradient windows / black bar instead of corner chip (screenshot). Fan-out itself OK;
units/DPI of child-viewport placement on multi-monitor Windows is the suspect (external
advice .tmp/toast-monitors.md: scenario 🅰 enumeration timing / 🅱 logical-vs-physical units
/ 🅲 rendering). Console debug output allowed by user.
- [x] Research (E): how to hide winit windows from taskbar with the locked stack (no new
  deps): thread backend owns the window (direct FFI via raw handle, precedent gl_boot);
  host backend child viewports — check vendored egui-winit/eframe 0.37 for any supported
  path (taskbar_icon semantics, WindowBuilder flags) and fallback options (GWLP_HWNDPARENT /
  WS_EX_TOOLWINDOW via EnumWindows title-match); decide unsafe confinement (overlay policy:
  module-scoped allow sites; virshot stays forbid). → research.md
  DONE (research.md:535): ZERO unsafe needed — egui 0.37 `ViewportBuilder::with_taskbar(false)`
  maps to winit `with_skip_taskbar` on Windows (egui-winit lib.rs:2443-2450); thread backend
  gets `WindowAttributesExtWindows::with_skip_taskbar(true)` (cfg windows). EnumWindows FFI
  WS_EX_TOOLWINDOW = fallback only if buttons reappear (would widen unsafe exception =
  user sign-off flag). GWLP_HWNDPARENT rejected. Windows checklist captured.
- [x] Research (F): diagnose vs screenshot+advice — count spawns vs rects is known (3
  spawns), so 🅰 ruled out; verify vendored how egui-winit applies ViewportCommand::SetPosition/
  SetInnerSize for deferred viewports on Windows (which pixels_per_point scales them — root vs
  per-monitor), what cc.pixels_per_point() is at CreationContext, MonitorHandle::scale_factor
  semantics; propose per-target-monitor conversion. → research.md
  DONE (research.md:702-910): 🅱 CONFIRMED — callers ÷ root ppf (= 1.0 at CreationContext),
  egui × child-window ppp (`zoom_factor × window.scale_factor()`, egui-winit lib.rs:59-63,
  2484-2530). Rule: builder/command values = physical ÷ ppp_of_that_window; authoritative
  correction via observed `native_pixels_per_point` (lib.rs:1496) re-applying
  OuterPosition/InnerSize on ppp change. Chosen strategy (c): physical rects end-to-end +
  `MonitorRect{rect,scale}` API. 🅲 (transparent child skips glutin_winit::finalize_window ⇒
  alpha pixel format unselected; no WS_EX_NOREDIRECTIONBITMAP) stays OPEN as separate defect —
  debug lines separate ghost vs painted-without-alpha. Mixed-DPI caveat RESOLVED.
- [x] Add debug console output (user-approved): per spawn log monitor rect logical+physical,
  target scale_factor, applied viewport position; toast_debug stderr + virshot tracing INFO.
  DONE: `OverlayHost::set_debug(bool)` (default off, AtomicBool) — lines: spawn (scale,
  physical, builder_logical), ppp correction (`ppp <old|-> <new>: re-apply`), observed
  physical + drift (only when |drift|>=1px ⇒ steady chip = no drift lines = placement landed);
  `observed_rect` helper + 3 overlay tests. toast_debug: set_debug(true), spawn line gains
  monitor index (pure `monitor_of`) + scale, label `monitors (physical)` + 2 example tests.
  virshot: startup `tracing::info!("Toast placement monitors: ...")` (name/pos/size/scale/
  primary via winit MonitorHandle::name()) + 2 gui tests; log→tracing bridge landed via
  `subscriber.try_init()` (tracing-subscriber 0.3.23 has no with_tracing_logs; try_init runs
  LogTracer under default tracing-log feature — no new dep, verified vendored util.rs:61-75).
  All gates green (overlay 41 tests, toast example 7).
- [x] Fix (E): taskbar entries hidden on Windows (thread backend + host backend per research
  decision); Linux unaffected; gates green.
  DONE: host.rs builder `.with_taskbar(false)`; gl_boot.rs cfg(windows)
  `with_skip_taskbar(true)` (shadowed attributes, mirrors egui-winit style); service.rs has no
  own attributes builder. Zero unsafe, zero new deps; verified vendored mapping (egui-winit
  lib.rs:2443-2450, winit platform/windows.rs:500). check.sh + clippy linux/windows-gnu green.
  Residual risk (button reappear after chip click / explorer restart) → FFI fallback flagged.
- [x] Fix (F): correct chip placement on non-primary/DPI-different monitors (conversion per
  research; pure unit tests for the conversion math); Linux Xvfb smoke unchanged; Windows
  re-run confirms chips in corners on every monitor.
  DONE: physical rects end-to-end. New `overlay::MonitorRect{rect(physical px), scale}` +
  `OverlayWindowSpec.scale`; OverlayHost: builder = physical ÷ spec.scale guess, authoritative
  `OuterPosition/InnerSize` correction from observed `zoom × native_pixels_per_point`,
  re-sent only when reported ppp changes (`HostWindow.applied_ppp`). toast: chip physical
  size = logical chip_size × monitor.scale, insets stay px; `ToastService::spawn(sink,
  monitors, wake)`. Callers: toast_debug `monitor_rects` (no division, scale attached),
  virshot `monitor_rects_physical` + `host_monitor_rects(cc)` (winit monitors, xcap
  capture-only). New/updated pure tests (overlay placement/host, toast placement/service,
  example, gui.tests). check.sh + clippy (linux & windows-gnu all-targets) + toast example
  tests + windows-gnu test --no-run green. Residual: interim frame before correction may be
  misplaced (ratio guess/ppp); WM clamp/WM_DPICHANGED convergence; thread backend no ppp
  feedback (initial placement exact only).
- [x] Gates + rebuild windows-gnu virshot.exe + toast_debug.exe + overlay_demo.exe + commit
  DONE: gui.rs `host.set_debug(true)` (stderr placement diagnostics, chips rare);
  check_full.sh exit 0 (fmt rewrote 6 Stage-7 files; 255+9 tests green); clippy all-targets
  linux + windows-gnu exit 0; windows-gnu test --no-run + toast_debug example exe compiled;
  ui-verify.sh all exit 0 ALL PASS (chip window present, documented Activate SKIP);
  Linux Xvfb smoke of toast_debug logged spawn→re-apply→hide lifecycle. Release artifacts:
  virshot.exe 8 139 776 B (<10 MB), overlay_demo.exe 6 278 656 B, toast_debug.exe 6 835 712 B.

**Criterion:** Windows — no overlay/toast windows in taskbar; chip visible in correct corner
on EVERY monitor (debug stderr shows per-monitor rect/scale); screenshot exclusion still holds.
Linux — gates + ui-verify green.
→ Linux half met (gates + ui-verify + Xvfb smoke); Windows acceptance = user re-run pending
(three rebuilt binaries; if chips still invisible with drift=(0,0) → 🅲 presentation defect
is the next lead).
**Commit:** `fix(overlay,toast): taskbar-invisible chips, per-monitor DPI placement, debug output`

---

## Stage 8 — Reusable monitor enumeration + primary monitor (user follow-up 3, 2026-11-05)
- [x] Research (G): winit `MonitorHandle` API — is "primary" exposed anywhere? enumeration
  order on Windows; duplication map between `toast_debug::host_monitors` and gui.rs
  (`monitor_rects_physical`, `monitor_metrics`); overlay crate can use `winit::window::Window`
  (dependency already present). Append findings to research.md.
  DONE: research.md "Stage 8 research (G)" (:912-1169). `Window::primary_monitor()`/
  `ActiveEventLoop::primary_monitor()` exist (safe); MonitorHandle has no is_primary,
  derives PartialEq; Windows enumeration order has NO contract (EnumDisplayMonitors callback)
  ⇒ we reorder primary-first ourselves; fallback heuristic = entry at exactly (0,0);
  Wayland primary = None; overlay has direct winit dep (Cargo.toml:10) so monitors.rs can
  take &winit::window::Window; duplication map + consumer plan recorded (§5-§8).
- [x] `crates/overlay/src/monitors.rs`: reusable enumeration — pure entry builder (raw tuples),
  from-winit-window function, single-monitor fallback, debug line with `primary` flag,
  primary detection; TDD unit tests.
  DONE: monitors.rs (353 lines) + `pub mod monitors;` in lib.rs. API: MonitorEntry
  {x,y,width,height,scale,is_primary,name}, entries() pure builder, from_winit_window()
  (primary_monitor equality, exact-(0,0) fallback flag), rects() (zero-size skip, scale attach,
  PRIMARY FIRST ⇒ MonitorSel::Index(0) = primary), debug_line() mirroring gui's line wording +
  sentinels, fallback_rect()/fallback_rect_from_points(). 9 pure tests, TDD red→green;
  check.sh EXIT=0; clippy linux + windows-gnu -D warnings clean; overlay tests 50 passed.
- [x] toast_debug: `host_monitors` delegates to `overlay::monitors`; stderr output shows the
  primary flag; when "all monitors" is UNCHECKED the chip targets the PRIMARY monitor
  (index of the primary entry), not enumeration index 0.
  DONE: example-private monitor_rects + raw enumeration + both inline fallbacks deleted;
  host_monitors = thin adapter (from_winit_window → rects + debug_line; fallback_rect /
  fallback_rect_from_points preserve old fallback order). stderr "toast-debug: monitors
  (physical): ... primary" via debug_line; unchecked All = Index(0) = PRIMARY (rects order,
  documented). monitor_of + spawn log kept verbatim. Example tests 3 (new debug_line primary
  test); check.sh EXIT 0; clippy workspace + windows-gnu EXIT 0; windows-gnu example test exe
  compiles; Xvfb smoke printed primary line + chip on monitor=0 (timeout SIGTERM expected).
- [x] gui.rs: replace duplicate monitor code with `overlay::monitors` (physical rects + debug
  line); keep the startup `Toast placement monitors:` tracing line; gui tests updated.
  DONE: deleted monitor_rects_physical, MonitorInfo, monitor_metrics, monitor_metrics_line,
  monitor_debug_line (:1096-1277). host_monitor_rects = thin adapter (cc.winit_window() →
  overlay::monitors::from_winit_window once → rects + debug_line; zero-size WARN with count;
  fallback_rect / fallback_rect_from_points preserve old wording/arith; no winit type named).
  Startup `Toast placement monitors: {placement_line}` kept. Primary ordering now OS
  primary_monitor() (intended improvement), not window.current_monitor(). Removed 6
  gui.tests monitor tests (logic covered in monitors.rs). check.sh + clippy linux &
  windows-gnu + windows-gnu test --no-run EXIT 0.
- [x] Gates: check_full, clippy (--workspace --all-targets linux + windows-gnu), windows-gnu
  `test --no-run` + example exes, ui-verify.sh all; rebuild windows-gnu release artifacts;
  commit.
  DONE (acceptance agent, independent): grep audit — enumeration code lives ONLY in
  crates/overlay/src/monitors.rs, both consumers wired via overlay::monitors; primary-first
  semantics verified (rects() index 0 = flagged entry, unchecked All = Index(0) documented
  PRIMARY); check_full EXIT 0 (fmt rewrote monitors.rs + toast_debug.rs); clippy linux +
  windows-gnu EXIT 0; windows-gnu test --no-run + toast_debug example EXIT 0; release exes:
  virshot.exe 8 143 872 B (<10 MB ✓), overlay_demo.exe 6 278 144 B, toast_debug.exe
  6 842 880 B; ui-verify.sh all ALL PASS (chip window, documented Activate SKIP); Xvfb smoke
  shows `monitors (physical): ... primary` + spawn monitor=0; gui startup line
  `Toast placement monitors: {placement_line}` fed by debug_line (meaning unchanged).
  Residual: primary-first reorder proven by tests + Linux single-monitor smoke; live
  multi-monitor Windows confirmation = user re-run.

**Criterion:** ONE monitor enumeration implementation shared by overlay/toast_debug/virshot;
toast_debug with "all monitors" unchecked places the chip on the PRIMARY monitor (debug line
marks which entry is primary); virshot's startup monitor line unchanged in meaning; all gates
green on Linux + windows-gnu compile.
→ Met (Linux side verified; Windows multi-monitor primary behavior = user re-run pending).
**Commit:** `refactor(overlay,toast,virshot): reusable monitor enumeration with primary detection`
