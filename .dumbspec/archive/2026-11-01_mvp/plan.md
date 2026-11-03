# 2026-11-01_mvp: implementation plan (VIRShot MVP)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

Verification gate everywhere: `./cargo.sh check && ./cargo.sh test && ./cargo.sh fmt && ./cargo.sh clippy -- -D warnings` (linux) plus `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` (compiles; Windows test binaries run on Windows hosts). Scratch only in `./.tmp/`.

---

## Stage 0 — Research + spec + plan
- [x] research pass: docs/spec.md MVP items enumerated, PoC coverage table, fixed constraints, risks
- [x] spec.md v1 (scope/approach/constraints/open questions/changes), user approvals recorded (hotkey crate, image jpeg+webp, instance-guard crate)
- [x] plan.md written

**Criterion:** research.md, spec.md v1, plan.md exist in `current/2026-11-01_mvp/` and scope matches docs/spec.md §2.
**Commit:** `docs(spec): MVP research, spec v1 and implementation plan`

## Stage 1 — Foundation: crate hygiene, module split, logging
- [x] `#![forbid(unsafe_code)]` at crate root; split PoC `src/main.rs` into per-module files per AGENTS.md (`config`, `template`, `capture`, `logging` start real; wiring stays in `main.rs`)
- [x] add explicit `image` dependency with features `jpeg`+`webp` (user-approved)
- [x] TDD `logging`: `--log-level` parser (DEBUG/INFO/ERROR, invalid → typed error); tracing file appender init (`virshot.log` next to config)
- [x] per-module `thiserror` enums; drop PoC `println!`/`eprintln!` paths in favor of tracing

**Criterion:** gate green on linux; windows-gnu compiles; forbid attribute present; one module per file.
**Commit:** `refactor(core): split PoC into modules, forbid unsafe, typed errors, logging module`

## Stage 2 — config module
- [x] red test: defaults match spec §7 (save_path, both templates, format=png, jpeg/webp quality=90, overlay enabled/BottomRight/2000/true)
- [x] serde schema `[general] [controls.full_screen] [controls.window] [controls.rect] [overlay]`; load/save round-trip
- [x] broken/unparsable config → log error + run on defaults
- [x] OS path resolution: Windows `%APPDATA%\virshot`, Linux `$XDG_CONFIG_HOME`/`~/.config/virshot` (manual env, no new crate); `--config` override deferred to Stage 5 CLI wiring
- [x] auto-create default config on first run (corrupt file NOT rewritten — ERROR + defaults; parent dir created; INFO "created default config at …")

**Criterion:** config unit tests green on both targets; defaults verbatim per spec §7:178-208.
**Commit:** `feat(config): OS-convention config.toml with defaults and error fallback`

## Stage 3 — template module
- [x] red tests: token rendering with injected `DateTime` — `%Y %y %mo %d %H %M %S %iso %t %n %ext` (`%mo`=2-digit month, `%iso`=YYYY-MM-DD_HH-mm-ss local, `%t`=title/`fullscreen`)
- [x] red tests: sanitization — invalid chars in `%t` → `-`, empty title → `untitled`, truncate ~60 chars (token value only), segment sanitize (`<>:"/\|?*` + control chars → `-`; `/`/`\` in tokens never create segments)
- [x] red tests: validation — reject absolute paths (leading `/`, `\`/UNC, drive prefix) and `..`, reject unknown tokens (strict names), accept relative from `save_path`
- [x] red tests: collisions — overwrite forbidden; `collision_variant` inserts `-2`/`-3` before extension of final segment (incl. dotted names); `%n` renders counter value
- [x] implementation (`src/template.rs`: `render(template, now, title, ext, counter)` + `collision_variant`, typed thiserror AbsoluteTemplate/ParentSegment/UnknownToken); subfolder auto-creation lives in the Stage 4 save path (caller joins + `create_dir_all`)

**Criterion:** template/sanitization/validation/collision unit tests green on both targets. **Done:** commit `b0427d2` — 32 template tests, full suite 72 green, windows-gnu compiles.
**Commit:** `feat(template): filename tokens, sanitization, validation, collision handling`

## Stage 4 — capture module
- [x] red tests: format→ext mapping (`Format::extension()` in config) + encode png/jpg/webp (synthetic image → decoded back, valid file); jpeg quality asserted (q10 < q90)
- [x] full-screen = monitor of foreground window, fallback primary (warn); own windows excluded via pid==process::id(); foreground = `xcap::Window::is_focused()` (Windows GetForegroundWindow; linux _NET_ACTIVE_WINDOW/XCB best-effort)
- [x] window capture via `xcap::Window::capture_image()` (occlusion-independent, PrintWindow on Windows); typed `NoActiveWindow` "no active window", file not saved; template/collision errors typed (`Template`, `TooManyCollisions` bound 1000)
- [x] capture thread: `spawn_capture(CaptureMode, owned Settings)`, `Local::now()` stamped at module boundary; collision disk-scan (re-render on `%n`, else `collision_variant`), overwrite forbidden; `create_dir_all` for template subfolders; no shared `Mutex`
- [x] webp deviation (v4.2): lossless VP8L only in `image` 0.25; `webp_quality` stored, unhonored; lossy needs libwebp/`wp` dep — user approval pending

**Criterion:** capture unit tests green on both targets; linux smoke capture of a real monitor works under WSLg/X11; windows-gnu compiles. **Done** (commit `6b8886e`, 22 new tests, suite 86 passed + 1 ignored): real-monitor smoke `#[ignore]`d on this host — no X11 socket (WSLg Wayland-only); run `cargo test -- --ignored` on a desktop host and verify PrintWindow/foreground on Windows hardware.
**Commit:** `feat(capture): full-screen/window capture, png/jpg/webp, save thread`

## Stage 5 — CLI + headless `virshot shot`
- [x] clap: `--config --log --log-level --version --help`; subcommand `shot [--window] [--out <path>] [--format png|jpg|webp]` (clap ValueEnum → config::Format)
- [x] headless mode: no GUI/tray/overlay; config default source; `--out`/`--format` override; saved path to stdout; exit 0 success / 1 error; usage errors exit 2 (clap; documented)
- [x] decisions: `--out` = exact output file path implemented via save_path=parent + filename-as-template so capture owns the pipeline; collision `-N` still applies, overwrite forbidden; `--format` selects encoding, filename kept verbatim; `VIRSHOT_LOG_LEVEL` env override replaced by `--log-level`; log parent dir bootstrapped before init_logging; 200 ms drain grace so final log line lands before exit
- [x] E2E regression in `tests/cli_headless.rs` (no-TDD): version/help/shot-help, headless failure path (exit 1, stderr+log, config auto-created, stdout empty, no file written), format/out parsing, clap exit-2 cases, default-log sibling; success-path E2E `#[ignore]`d — needs desktop host (X11/XWayland) or Windows

**Criterion:** E2E passes on linux headless; CLI contract per spec §8. **Done** (commit `c71a385`: 9 E2E green + success-path ignored; suite 95 passed, 2 ignored; windows-gnu binaries compile). Real-capture success path verified later on Windows/desktop acceptance (Stage 12 checklist).
**Commit:** `feat(cli): clap CLI with headless virshot shot and E2E test`

## Stage 5.5 — PoC: wgi backend + raw index logging (spec v2, user decision)
- [x] switch `gilrs` to `default-features = false, features = ["wgi"]`; log every button press; hardware result (user, Windows, generic HID pad vendor=1118 product=2835): connect/disconnect arrive, **zero button/axis events, no Dropped** — gilrs-core wgi gates on `old.time()==new.time()` and generic HID pads report a constant timestamp → presses invisible via gilrs. Resolution: custom Windows backend on `windows` crate RawGameController (user-approved spec v3), gilrs kept for Linux; **v4 (2026-11-02): gamepad deferred out of MVP** — PoC code removed from repo, user keeps own PoC outside; slot reserved (config `pad` field, UI bind field)
- **Commit:** `feat(gamepad): switch gilrs to wgi backend with raw button-code logging` (+ diag `0764407`)

## Stage 6 — Hotkeys: crate research + bindings model
- [x] `global-hotkey` v0.9.0 added (user-approved, no features); verified API from registry source: `GlobalHotKeyManager::new()/register()` (no `HotKeysManager`/`set_shortcuts`), events via `GlobalHotKeyEvent::receiver()` blocking crossbeam recv (`Pressed|Released`), keys = `keyboard_types::Code` (PrintScreen, KeyA..Z, Digit0..9, F1..F12), mods `ALT/CONTROL/SHIFT/META` (Win/Command→META); linux backend = X11 with crate's own pump thread, QUIRK: `register()` silently returns Ok when pump thread died (no X server) → our cfg linux probe checks `$DISPLAY` + `/tmp/.X11-unix/X<n>` before registering
- [x] key notation for config schema: `key` = crate Code names ("PrintScreen", "A".."Z", "0".."9", "F1".."F12", case-insensitive), `mods` = "alt"/"ctrl|control"/"shift"/"win|windows|command|meta|super" → META; typed `bindings::Error::UnknownKey/UnknownMod`; combo Display "alt+PrintScreen"
- [x] TDD `bindings` (36 tests, red→green): `Action{FullScreen,Window,Rect}`, rect typed reserved (parsed strictly, never registered/fires), pad inert (Stage 7 slot), `Debounce::try_fire(action, now)` 250 ms injected clock — exactly-250ms allowed, blocked repeat does not shift window, per-action independence
- [x] wire hotkey fire → capture: `hotkeys::start(settings)` on dedicated thread (manager on that thread for Windows WM_HOTKEY affinity), status via `sync_channel(1)` (start never hangs), event loop → debounce → `capture::spawn_capture`; typed `Registration{Registered/Failed/Reserved}` + `Error{Unavailable/Manager/Binding}`; headless host demo: typed ERROR "no X11 server socket", app stays resident, no panic
- [ ] Windows message pump on manager thread (crate does not pump; hand-written pump needs unsafe — forbidden): resolution expected via eframe/winit in Stage 8 (thread-affinity decision point: manager may need to move onto the GUI/main thread)

**Criterion:** bindings unit tests green on both targets; crate builds linux + windows-gnu; registration error surfaces as UI-ready status without panic. **Done** (commit `bf2a427`: 131 passed / 2 ignored, windows-gnu compiles).
**Commit:** `feat(hotkeys): global hotkey crate and bindings model with debounce`

## Stage 7 — Gamepad integration — **DEFERRED** (out of MVP — user decision 2026-11-02)
Future design (slot reserved: config `pad` field, UI bind field): Windows = own `gamepad` module — polling loop over HID via `gameinput.dll` (`windows` crate `RawGameController`); Linux = gilrs thread. Deps (`windows`, `gilrs`) re-added with user approval when this stage lands; user's own hardware PoC exists outside this repo — on success its branch/solution is copied in here.
- [ ] (deferred) `gamepad` module: Windows = own polling thread (poll ~8 ms, diff button STATES not timestamps — fixes generic HID pads with constant timestamps per Stage 5.5 finding), button labels via `GetButtonLabel`; Linux = gilrs thread; both emit unified event (raw index + label) → binding match → command to main
- [ ] (deferred) plug-and-play: connect/disconnect INFO, thread survives unplug; connected-pads info for settings UI (Stage 8)
- [ ] (deferred) tests with synthetic events: pad-button binding fires action; multi-button combos unsupported; debounce applies to pad triggers
- [ ] (deferred) config pad notation parsing/matching: canonical `gilrs::Button` variant name (Linux) OR `unknown:<N>` raw index (Windows: raw index from RawGameController, UI shows `GetButtonLabel`); bind-by-press writes `unknown:<N>`; TDD for notation parse + match

**Criterion (when implemented):** synthetic-event tests green on both targets; hardware press events verified on Windows (incl. Share via `unknown:<N>`).
**History:** PoC commits `e041e33`/`0764407`/`76c19a8` (wgi diagnostics, custom RawGameController backend) — MVP code removed 2026-11-02; the Stage 5.5 hardware finding stands.
**Commit:** `feat(gamepad): input thread with binding-driven actions and plug/unplug INFO`

## Stage 8 — Settings window (eframe GUI)
- [x] bordered ~720×520 resizable window; tabs General / Hotkeys / Overlay, UI English (eframe/egui 0.37, glow)
- [x] Apply → `config::save` + live apply: synchronous hotkey re-registration on MAIN thread on both platforms (deviation from controller-channel design: WM_HOTKEY thread affinity requires manager on the winit-pumped main thread; linux manager object stays on main, its pump thread is crate-internal); Settings → listener via `Mutex<Settings>` config-mailbox (owned clone per trigger — permitted mailbox shape, not shared capture state); listener re-parses controls per event so re-registered combos honored immediately; close-without-Apply discards draft
- [x] bind-by-press per row (egui Key events → key+mods; Escape cancels; unsupported ignored; Win key checkbox-only — winit exposes no Super; PrintScreen press may also fire the registered hotkey — documented quirk); registration status per row via `hotkeys::status_line` (Registered/Failed/Reserved), no crash; gamepad bind capture + connected-pads list deferred with Stage 7; folder browse omitted (would need unapproved `rfd`)
- [x] X-close hides: `ViewportCommand::CancelClose` + `Visible(false)`, loop keeps running (verified in epi_integration source), draft reset to applied; reopen needs tray Stage 10; GUI startup failure → typed `gui::Error` → idle park fallback (headless contract untouched)

**Criterion:** settings window runs on linux (WSLg/X11); Apply persists + re-registers; windows-gnu compiles. **Done** (commit `8cb4d75`: 141 tests — 130 unit + 10 integration, 2 ignored; windows-gnu eframe/glow compiles zero-warning). Runtime smoke on WSLg Wayland: process resident 12s, window creation best-effort (MESA warnings, no crash), typed hotkeys-unavailable log; visual verification + Windows runtime pending user acceptance (Stage 12 checklist).
**Commit:** `feat(gui): settings window with 3 tabs, Apply and bind-by-press`

## Stage 9 — Overlay toast
- [x] egui multi-viewport toast (`show_viewport_deferred` — no `ctx.add_viewport`; builder patch diffs per frame): `with_transparent(true)` (+ root builder transparency — GL alpha config derives from ROOT viewport; settings window stays opaque via CentralPanel), `with_mouse_passthrough(true)`, `with_always_on_top()`, `with_decorations(false)`; fade via `Color32::gamma_multiply` (no painter alpha in 0.37); native-window fallback NOT needed (spec research item resolved)
- [x] fade in/out 250 ms; hold = `duration_ms`; error 3000 ms always shown even when `overlay.enabled=false`; one toast, new replaces current (`ToastState::replace`); corners via `toast_position` helper 16 px inset (Wayland ignores position/always-on-top — compositor decides, documented; toast still shows); width from text 220..460 px
- [x] success text `Saved: <file name>` (full path in log INFO); error text verbatim; mailbox hand-off: `ToastMailbox = Arc<Mutex<Option<Notification>>>` latest-only (config-mailbox pattern precedent), capture pushes after core, GUI drives lifecycle in `logic()` (works while settings window hidden)
- [x] TDD overlay.rs (injected monotonic clock): 20 tests phase/alpha/boundaries/error-override/disable/replace + 6 gui geometry + 2 capture hand-off; `on_all_monitors` deviation recorded in spec (single toast on current monitor — egui 0.37 exposes no monitor enumeration to App; checkbox + config field kept)

**Criterion:** toast appears for real captures on linux test env; timing unit tests green; windows-gnu compiles. **Done** (commit `ee21435`: 158 unit + 9 E2E green, 2 ignored; windows-gnu builds). Live toast rendering unverifiable on this sandbox (no usable Wayland compositor/GPU — GUI-failure fallback path exercised instead); desktop/Windows acceptance deferred to Stage 12 checklist.
**Commit:** `feat(overlay): click-through toast with fade, corners, all-monitor mode`

## Stage 10 — System tray
- [x] tray-icon 0.26.1 (ksni): code-generated 32×32 camera glyph (`Icon::from_rgba`, no `Icon::new`), menu `Take screenshot / sep / Settings / Exit` (exact §4.5 labels), tooltip "virshot", `with_menu_on_left_click(false)`; typed `TrayUnavailable` on failure, app keeps running
- [x] event bridge: global crossbeam receivers `TrayIconEvent::receiver()` (NOT `IconEvent`; `#[non_exhaustive]`) + `MenuEvent::receiver()` polled via `try_recv()` in `logic()` each frame (no extra thread/Mutex); ksni emits Click{Left,Up} only (Down+Up on Windows → Up-only mapping avoids double-show); right-click menu is host-side via D-Bus protocol; events don't wake winit → `ctx.request_repaint_after(500ms)` keeps hidden-window polling alive
- [x] actions: left click/Settings → `Visible(true)`; Take screenshot → `spawn_capture(FullScreen, applied settings, mailbox)`; Exit → only path that shuts down (no CancelClose on exit pass — eframe 0.37 has no `App::should_exit`, verified); GUI-failure park polls tray too (capture works, Settings→WARN, Exit→AtomicBool shutdown, exit 0); tray created before GUI so it survives GUI failure
- [x] linux feasibility: session bus present on sandbox → "System tray ready"; real SNI host consumption + click/menu UX unverifiable in sandbox → Windows acceptance (Stage 12)

**Criterion:** tray actions trigger capture/settings/exit on linux test env where feasible; windows-gnu compiles; feasibility status documented. **Done** (commit `73ba9b0`: 165 unit + 9 E2E green, 7 new tray tests; windows-gnu compiles, ksni cfg-linux-only).
**Commit:** `feat(tray): tray icon, menu actions, event-loop bridge`

## Stage 11 — Single-instance guard
- [x] research decision: std-only lock-file (approved instance-guard crate stays unused — KISS); lock `<config>.lock` beside the config used by the run (`lock_path_for` pure helper); create_new + own pid; alive → typed `AlreadyRunning{pid}` exit 1 before tray/hotkeys/GUI; dead/corrupt lock → delete + recreate once (concurrent race → `StaleLockUnusable`, never silently second instance); Drop best-effort remove; SIGKILL leaves stale lock — recovery proven in smoke
- [x] pid liveness cfg-gated: linux `/proc/<pid>`, windows `tasklist /FI "PID eq <pid>"` (missing/failing tasklist = UNCERTAIN → honor lock, never panic), pure `pid_alive(pid, probe)` tested with injected probes; other platforms honor lock
- [x] scope carve-out decided + documented: guard protects the RESIDENT app only; headless `virshot shot` does not acquire it (scripted/E2E captures must work while app runs); two instances with different `--config` run concurrently by design; behavior change: config dir uncreatable now exits 1 (Error::Io) instead of running on defaults; LOG_DRAIN_GRACE added to guard-failure fast exit (log line reached)

**Criterion:** with one instance running, a second launch exits fast with a log line; unit/smoke on linux; windows-gnu compiles. **Done** (commit `431c3b1`: 174 unit + 10 E2E, 2 ignored; smoke: second instance exit 1 in ~1s with typed reason in log+stderr, shot not blocked, stale recovery after SIGKILL works). Windows tasklist probe compile-verified only (Stage 12 checklist item).
**Commit:** `feat(instance): single-instance guard`

## Stage 12 — Budgets, delivery, acceptance
- [x] release builds both targets; Windows exe 12.38 MiB → **7.58 MiB** after eframe trim (`default-features=false, features=["glow","default_fonts","x11","wayland"]` — accesskit platform/winit stacks pruned −682 lock lines; `x11/wayland` needed by winit 0.30.13 on unix; egui keeps accesskit non-optional — only platform crates dropped; `[profile.release]` opt-level z / lto thin / strip already in place; no new deps) — commits `3191121` + `71dc0be`
- [x] budgets measured (linux, release): park-fallback resident 9.2 MB RSS, 0.00% CPU over 12 s ✓; GUI-open here (Wayland software compositor) ~133 MB/1.8% — attributed to glow + software raster, Windows GPU background numbers are Stage-12 checklist item for user
- [x] portable zip `./.tmp/virshot-windows.zip` (untracked): virshot.exe 7.58 MiB + README.md, CRC OK
- [x] README.md (Russian, tracked): run/hotkeys/tray/config+log paths/templates/formats (webp lossless)/CLI contract/single-instance/MVP limitations
- [x] Windows manual acceptance checklist delivered in chat (Russian, 14 items incl. Win11 PrintScreen Snipping Tool note, stale-lock recovery, rebind via bind-by-press, budgets; gamepad/Share explicitly deferred)
- [x] final gate green both targets after trim: check, test (173 lib + 9 E2E, 2 ignored), fmt, clippy -D warnings, test --no-run windows-gnu

**Criterion:** release builds under budgets, zip + README delivered, checklist in chat, gate green both targets. **Done** — MVP Stages 1-12 complete (Stage 7 deferred by user decision); archive of .dumbspec awaits explicit user confirmation.

**Criterion:** win-gnu release binary < 10 MB; zip artifact exists; measurements recorded in chat; full gate green on both targets.
**Commit:** `feat(release): budget-checked release build and portable zip delivery`

---

## Stage 13 — Hotkeys tab polish (user refinements 1-2)
- [x] bind field = button showing current binding via pure `binding_label` (canonical Alt/Ctrl/Shift/Win order, case-insensitive input, unknown mods skipped); click → bind-by-press ("Press keys…", Esc cancels); captured key+Alt/Ctrl/Shift REPLACE draft, pad kept; key ComboBox + modifier checkboxes removed; gamepad placeholder kept; Win via config.toml only (quirk documented)
- [x] status icons instead of inline text: `icon_for_status(&Registration)` — Warning `!` (tooltip = typed reason via on_hover_text) / Info `i` ("reserved — out of MVP scope" via on_disabled_hover_text on disabled rect row) / none; ASCII glyphs because bundled font stack (Hack+Ubuntu-Light+egui-icons) lacks ⚠/ℹ — fontTools-verified; `status_line` deleted (dead), controller + main logging untouched
- [x] 3-column rows kept; "Hotkeys unavailable on this host" tail kept

**Criterion:** Hotkeys tab shows bind buttons + warning/info icons with tooltips, no selects/checkboxes; gate green both targets; UI smoke screenshot-less (host limits) — logic tests for status→icon mapping. **Done** (commit `224a099`: +15 tests, −2 widgets/+1 dead helper removed, 185 unit + 10 E2E, 2 ignored; UI visuals not screenshot-verifiable here → Windows acceptance).
**Commit:** `feat(gui): hotkey bind buttons with warning-icon tooltips`

## Stage 14 — save_path Browse dialog + default resolution (refinement 3)
- [x] user approval for `rfd` received (2026-11-03, 09:21); fallback choice approved: %APPDATA%\virshot when exe dir unwritable
- [x] rfd 0.17.2 added (default features xdg-portal+wayland; blocking FileDialog::set_directory().pick_folder() verified in locked source; Windows native common controls, Linux portal+zenity fallback; missing backend -> None, no panic; pollster locked unix-only) — Browse button in General row; Some -> draft save_path + INFO, None -> WARN "folder picker unavailable", draft unchanged, app alive; dialog modal/blocking documented
- [x] home-resolved default save_path: `home_from_var`/`resolve_home`/`home_dir()` (USERPROFILE|HOME), `default_save_path(home)` = Pictures/Screenshots, no-home fallback cwd-relative `Pictures/Screenshots` (mirrors FALLBACK_CONFIG_DIR convention); General::default via general_with_home; hardcoded path gone
- [x] `browse_start_dir(save_path, home)` pure — save_path if dir else home/Pictures else relative Pictures

**Criterion:** defaults resolve per user home; Browse writes dir into draft; Apply persists; gate green incl windows-gnu. **Done** (commit `c10bce0`: +11 tests (196 unit + 10 E2E, 2 ignored), E2E gained save_path home-resolution assertion; live Linux config got `$HOME/Pictures/Screenshots`; real dialog unexercised headless — Windows/desktop acceptance item).
**Commit:** `feat(gui): native folder picker for save_path, home-resolved default`

## Stage 15 — Portable config/log next to exe (refinement 4)
- [x] Windows candidates pure `windows_config_candidates` (exe dir → %APPDATA%\virshot → FALLBACK), `dir_writable` injected probe (`virshot.probe.tmp` create_new + best-effort remove), `windows_config_path` picks first writable dir, WARN on fallback; `default_config_path()` cfg-gated, unix branch verbatim
- [x] log already sibling of config path (logging.rs derivation verified — portable log free); instance lock `<config>.lock` unchanged (portable sibling test added)
- [x] `--config`/`--log` overrides never trigger fallback (clap evaluates default only when absent); first-run auto-create beside exe is compile-verified here (Windows runtime = acceptance item)

**Criterion:** windows candidates/probe logic unit-tested incl all-unwritable→fallback; linux behavior unchanged (XDG); gate green both targets. **Done** (commit `9044b9a`: +12 tests, 208 unit + 10 E2E, 2 ignored; linux smoke XDG unchanged, config+log+lock beside --config path).
**Commit:** `feat(config): portable config+log beside exe on Windows`

## Stage 16 — Re-deliver after refinements
- [x] README (126 lines): portable config+log beside exe (+fallback WARN), Browse native picker (Linux portal→zenity only per locked rfd), bind-button hotkey rows + !/i icons tooltips, Win-modifier config-only, home-resolved save_path, lock beside config, quality 1–100
- [x] exe 7,982,592 B (<10MB ✓; +27KB vs Stage 12 = rfd), zip 3,153,430 B (flat virshot.exe+README, integrity ok); gate green both targets (208 unit + 10 E2E, 2 ignored)

**Criterion:** zip rebuilt with updated README; gate green. **Done** (commit `7603235`; re-delivery checklist deltas sent in chat).
**Commit:** `docs(readme): portable paths and hotkey UI updates`

---

## Stage 17 — Windows subsystem WINDOWS (no black console)
- [x] `.cargo/config.toml`: new `[target.x86_64-pc-windows-gnu] rustflags = ["-C","link-args=-Wl,--subsystem,windows"]` (mingw form; last --subsystem wins over rustc's console default)
- [x] release exe PE subsystem 3 (CUI) → 2 (GUI), stdlib python parser + objdump cross-check (.tmp stashes before/after)
- [x] CLI stdout-only-from-console note deferred to Stage 20 README
- [x] linux unaffected (target-scoped; native builds/tests link fine); gate green

**Criterion:** built exe PE subsystem=2; linux unaffected; gate green. **Done** (commit `d46e469`; runtime "no black window" = user Windows check).
**Commit:** `build(windows): subsystem windows to suppress console on launch`

## Stage 18 — Unregister hotkeys during bind-by-press (PrintScreen rebind)
- [x] global-hotkey 0.9.0 facts (locked source): unregister(HotKey)/unregister_all(&[HotKey]) — aborts at first failure → per-combo best-effort loop + WARN; Manager NOT Send/Clone (HWND/ThreadMessage) → controller owns Box<dyn Registrar>, App already owned controller → pause/restore called on MAIN thread (manager thread, affinity kept); receiver survives unregister_all (static GLOBAL_HOTKEY_CHANNEL; hidden window/pump live until Drop)
- [x] controller.pause() (unregister live combos, INFO, idempotent) / restore() (re-register LAST APPLIED from mailbox — draft untouched until Apply, INFO + per-combo ERROR); apply() clears paused flag; gui pure `capture_transition(capturing, Click|Escape|Commit)` drives Pause/Restore; tooltip quirk wording dropped
- [x] 8 tests (MockRegistrar state machine + transition mapping), 216 unit + 10 E2E, 2 ignored; gate green both targets

**Criterion:** pressing PrintScreen during capture-press reaches the app (no hook steal); restores after commit/cancel; gate green. **Done** (commit `84d27a7`; Windows runtime = user check — during capture bare PrtScn still copies to clipboard, Windows default).
**Commit:** `feat(hotkeys): pause global hotkeys during bind-by-press`

## Stage 19 — Toast on all monitors, only after screenshot
- [x] root cause FOUND (source): hidden root window → `Context::run_logic` emits logic output only; glow gates FULL pass on visibility → deferred viewport builders never delivered → toast window never created. Fix: `ViewportCommand::RequestPaintWhileHidden` to ROOT each pass while toast alive (egui-winit wants_paint → full pass; invisible-window paint throttled 100ms → fade coarser ~250ms@~10fps, acceptable). Transparency already on viewport builder (+root inheritance); `with_always_on_top_dd` doesn't exist — real API `with_always_on_top()` (kept); added `with_visible(true)`; Windows window-alpha unverifiable → solid chip on transparent window kept.
- [x] per-monitor: `MonitorRect` logical pts (xcap px ÷ scale_factor), pure `toast_positions(monitors, corner, size, offset, on_all_monitors)`; xcap→egui monitor_size→unpositioned fallback chain (WARN, no crash); replacement = close old ids (`ViewportCommand::Close`, generation ids — egui 0.37 doesn't auto-close abandoned viewports); INFO "Toast shown on N monitor(s)" + DEBUG close lines.
- [x] trigger unchanged (post-capture result only, errors even when overlay off); 4 new tests → 219 unit + 10 E2E (2 ignored); gate green both targets.

**Criterion:** toast viewports created per monitor with logging; trigger unchanged; gate green. **Done** (commit `8294d53`; Windows visibility = user check — look for INFO "Toast shown on ..." in virshot.log).
**Commit:** `feat(overlay): per-monitor toast placement`

## Stage 20 — Rebuild exe for user testing (no zip)
- [x] README (Russian): run-from-build delivery (no zip), no black console + CLI-stdout-from-console caveat, hotkey pause during bind-by-press (+ bare PrtScn clipboard note), toast only-after-screenshot per-monitor + "Toast shown on ..." log hint; stale multi-monitor limitation bullet removed
- [x] fresh release exe: 8,010,752 B (<10MB ✓), PE subsystem=2 verified; gate green both targets; user Windows test round scheduled

**Criterion:** fresh exe built, user test round scheduled. **Done** (commit `41b9835`; awaiting user report on items 1-3).
**Commit:** `docs(readme): console subsystem and toast notes`

---

## Stage 21 — Toast paint robustness (shadow-only fix)
- [x] verdict: white shadow = toast window NEVER painted (repaint request in spawn pass → Wait + repaint_delay pinned ZERO dedups later); "two windows" = replacement pass (eframe creates new before destroying old) — partial edits (FadeIn removed) kept and extended
- [x] fix: one window per monitor — replacement closes then spawns NEXT pass (toast_window_step pure mapping); no inner-color in egui 0.37 → hidden+paint-while-hidden, Visible(true) after first painted frame; AOT → WS_EX_TOPMOST chain; with_active(false) → SW_SHOWNOACTIVATE
- [x] 12 new tests (231 unit + 10 E2E, 2 ignored); gate green both targets

**Criterion:** toast content drawn from frame one; no blank/unpainted window path; gate green. **Done** (commit `b431aa4`; Windows visibility = user re-check).
**Commit:** `fix(overlay): single always-on-top toast window`

## Stage 22 — Corner offsets 30/60 (taskbar-aware empirically; windows-sys REJECTED by user — unsafe)
- [x] TOAST_OFFSET_X=30/TOAST_OFFSET_Y=60 replace 16px inset (tk-reference values; bottom corners clear ~48px taskbar); helpers take per-axis f32; clamp/on_all_monitors/INFO unchanged
- [x] 7 placement tests updated/re-anchored (counts 231 unit + 10 E2E, 2 ignored); gate green both targets

**Criterion:** toasts sit 30/60 px from monitor borders (above taskbar at bottom corners); gate green. **Done** (commit `18e92bc`; Windows placement = user check).
**Commit:** `feat(overlay): empirical corner offsets 30/60`

## Stage 23 — Rebuild + README v4 + user test
- [ ] rebuild release windows exe after Stages 21-22; README toast bullets: taskbar-aware placement, painted-from-first-frame note
- [ ] deliver exe from build; user re-tests toast (shadow + taskbar)

**Criterion:** fresh exe, README updated, user test scheduled.
**Commit:** `docs(readme): toast taskbar-aware placement`

