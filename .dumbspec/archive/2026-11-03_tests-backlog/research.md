# Research: 2026-11-03_tests-backlog

Superficial context collection for the three backlog tasks (unit tests relocation,
xvfb E2E, memory profiling). Findings appended as discovered; no solutions designed here.

## R1 — Current test layout (unit tests inside src files)

- Modules in `src/` (one module per file; wired in `src/main.rs:59-68`): `bindings.rs`,
  `capture.rs`, `config.rs`, `gui.rs`, `hotkeys.rs`, `instance.rs`, `logging.rs`,
  `overlay.rs`, `template.rs`, `tray.rs`. Every module except `main.rs` has exactly one
  `#[cfg(test)] mod tests` (bindings:263, capture:423, config:516, gui:1466, hotkeys:505,
  instance:188, logging:184, overlay:180, template:197, tray:246). Total `#[test]` count:
  **232** (231 pass + 1 ignored; bindings 21, capture 22, config 46, gui 49, hotkeys 17,
  instance 9, logging 8,
  overlay 21, template 32, tray 7). Names follow `<unit>_<scenario>_<expectation>`.
- `tests/` today: only `tests/cli_headless.rs` — 9 E2E tests (8 active + 1 `#[ignore]`d
  desktop-success at :286), invoke the binary via `env!("CARGO_BIN_EXE_virshot")` (:22),
  assert exit codes / stdout / auto-created config / log content; scratch dirs in `./.tmp/`.
- **The crate is binary-only** (`src/main.rs` crate root, `#![forbid(unsafe_code)]` at :57,
  no `src/lib.rs`, no `[lib]` in `Cargo.toml`): integration tests cannot `use virshot::…`
  at all today. Tested items are mostly `pub(crate)` (template `render:64`,
  `collision_variant:93`; config `load:93`, `load_or_create:139`; logging `parse_level:59`;
  bindings `parse_controls:120`; tray `camera_glyph_rgba:78`, `action_for_click:133`;
  gui `toast_position:230`, `supported_key_name:665`) **plus many private helpers tested
  directly** (template `is_absolute:107`, `render_segment:117`, `token_value:150`,
  `sanitize_segment:182`; instance `write_lock:87`, `read_lock_pid:97`, `lock_path_for:147`,
  `pid_alive:155`; capture `unique_path:319`, `save_image:361`, `encode_image:386`; config
  `line_of:51`, `dir_writable:231`; bindings `parse_binding:130`, `parse_key:153`; gui
  `toast_window_step:447`; hotkeys `register:336`, `backend_status:460`; tray `fill_rect:94`).
  Some tests inject probes/closures into private fns (`dir_writable`, `pid_alive`) and use a
  `cfg(test)` mock `Registrar` trait (`hotkeys.rs:81`).
- `Cargo.toml`: no `[[test]]` sections, no harness overrides; `[dev-dependencies]` =
  `rusty-hook` only.
- Gates: `./scripts/check.sh` = `cargo check` + `cargo test` + `cargo clippy -D warnings`
  (linux, runs unit + E2E). Windows gate `./cargo.sh test --no-run --target
  x86_64-pc-windows-gnu` **does compile integration tests** (`cli_headless-*.exe` present in
  the windows-gnu debug deps), so any `tests/unit/` file must cross-compile there too.
- Placement constraints (fixed, from project docs):
  - AGENTS.md "Code structure": module tests go into that file's `#[cfg(test)] mod tests`
    or a sibling `src/<module>.tests`-style file — **never into an unrelated file**.
  - AGENTS.md "Best practices": "keep module interfaces tight: `pub(crate)` by default".
  - AGENTS.md "Testing": unit = pure logic, TDD; must pass linux gate and windows-gnu
    compile; E2E lives in `tests/`, no TDD.
  - `docs/spec.md` §10: unit = template/sanitization/validation/config parsing; integration
    = headless `virshot shot`; Windows acceptance manual. No mention of `tests/unit/`.

### R1 constraints for the move

- Moving in-file tests to `tests/unit/` requires a **lib target** (`src/lib.rs` re-declaring
  the 10 modules) — a crate-structure change not covered by the dependency-approval rule.
- `pub(crate)` items are unreachable from `tests/`; nearly every tested item is
  `pub(crate)` or **private**, so the move forces widening to `pub` (or keeping tests
  in-file), which conflicts with the "tight interfaces" rule.
- `tests/unit/` contradicts the current AGENTS.md placement convention unless that
  convention is amended first (user decision needed).
- Windows-gnu gate compiles integration tests: moved tests must cross-compile
  (egui/xcap types, cfg(test) mocks).
- New dev-dependencies (e.g. `tempfile`) would need explicit user approval.

### R1 gaps

- Cross-module test coupling per file (which private helpers a test pulls from another
  module) not exhaustively traced.
- The sibling `src/<module>.tests` style named by AGENTS.md is not used anywhere yet —
  all tests are in-file today.
- Whether the lib-target/visibility-widening structural change needs user approval is
  unaddressed in AGENTS.md.

## R2 — `.tmp/qa-harness/` and xvfb feasibility (Linux only)

Re-scoped by user 2026-11-03: not E2E tests — helper scripts in `./scripts/` for manual
testing so the agent can *see* the app. (Correction: an earlier version of this section was
written before the R2 report arrived and stated xvfb was absent; the verified facts below
replace it.)

### ENVIRONMENT (exact outputs)

- `which xvfb-run` → `/usr/bin/xvfb-run`; `which Xvfb` → `/usr/bin/Xvfb`;
  `dpkg -l xvfb` → `2:21.1.12-1ubuntu1.6` → **Xvfb is installed**.
- `DISPLAY=:0`, `WAYLAND_DISPLAY=wayland-0`, `XDG_SESSION_TYPE` unset,
  `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`; `/tmp/.X11-unix/` does **not**
  exist → no real X server on the host (hotkey probe reports Unavailable). Xvfb creates
  the socket `/tmp/.X11-unix/XN`, so X11 paths become reachable only inside an Xvfb run.
- Installed and usable: `xwininfo`, ImageMagick `convert` / `compare` / `import`, `python3`,
  Mesa (`libegl-mesa0`, `libgl1-mesa-dri`, `libegl1-mesa-dev`).
- Missing: `xdotool`, `scrot`, `openbox`, `wmctrl`, `grim`, `spectacle` → any of these is a
  **new native package** and needs user approval.
- Screenshots without new deps: ImageMagick `import -window root` on the Xvfb display works
  (verified on the same Xvfb where xcap failed).

### HARNESS (`.tmp/qa-harness/` — vendored clone of github.com/grymmjack/qa-harness)

- `bin/qa` — loader: `source drivers/linux-x11/driver.sh; source <adapter>/manifest.sh;
  source core/runner.sh; qa_main "$@"`; usage `bin/qa --adapter adapters/rust-demo --offscreen`.
- `core/runner.sh` — bash test files with a DSL (`info/skip/region/snap/assert_*`), per-run
  `RESULTS_DIR` (`run-<ts>.log`, RESULTS_TSV), JUnit + HTML report, non-zero exit, live
  `--status/--json`, `--mode offscreen|onscreen|ask`, `--calibrate` (outlines regions on live
  UI), `--rerun-passed` cache. Cleanup is PID-scoped (`_qa_clean_dead_snaps` deletes only
  `…_<PID>.png` whose PID is dead); `trap _qa_on_exit EXIT` + INT/TERM; `_kill_tree` kills
  children first.
- `core/assert.sh` — pixel asserts, ImageMagick only: `convert "$tmp" -crop WxH+X+Y +repage`
  (fullscreen → client-area → sub-region), `compare -metric AE -fuzz 2%`,
  `assert_regions_differ` / `assert_regions_same` (default tolerance 1500 px),
  `assert_window_exists/title`, `assert_no_crash`; on failure saves `-where.png` with the
  region boxed red (`-stroke red -draw "rectangle …"`).
- `core/input.sh` — app-agnostic `click/drag/key/type/wait_for` delegating to driver verbs.
- `drivers/linux-x11/driver.sh` — requires `xdotool scrot xwininfo convert compare`; input via
  xdotool (XTEST), capture via `scrot`, bounds via `xwininfo -id`; offscreen wrap:
  `env -u WAYLAND_DISPLAY xvfb-run -a --server-args="-screen 0 ${res}x24" bash -c 'openbox
  --sm-disable --config-file openbox-rc.xml & …; exec "$@"'`; bundled `openbox-rc.xml`
  UNDECORATES windows so region geometry is pixel-identical; knobs `QA_XVFB_RES`,
  `QA_OFFSCREEN_WM`, `QA_CAPTURE`.
- `adapters/adapter.template.sh` — the whole per-app surface (6 sections): `adapter_launch`,
  `adapter_quit`, `adapter_window_bounds` ("X Y W H", match by title/pid), `adapter_to_screen`
  (logical→screen seam), `ADAPTER_CONFIG` + `adapter_ensure_config` (isolated config,
  re-asserted before every test file), `adapter_is_alive` / `adapter_park_window` /
  `adapter_crashlog_dir` / `adapter_pre_input`.
- `adapters/rust-demo/` — ~30-line manifest proving the core works against a Rust GUI:
  `target/release/qa-demo &`, poll `xdotool search --name qa-demo` (0.1s steps), quit =
  `kill $ADAPTER_PID` + `pkill -x qa-demo`, bounds by title, `adapter_focus`
  (`xdotool windowfocus --sync` + `windowraise`) because a WM-less Xvfb never sets focus.
- `adapters/kaleidotron/` — 38 tests for a Rust/eframe app. Ideas worth borrowing: sandbox
  HOME re-made per launch (`rm -rf; mkdir config,data,cache`), title PREFIX match,
  "deliberately NOT `pkill -x <app>`" (blanket pkill would kill the developer's instance),
  `known-failures.txt` + `qa-target.txt`, exported `KT_WIN_W/KT_WIN_H` as coordinate space.
- `report/qa-report.py`, `dash/qa-dash.py` — TSV-driven HTML/JUnit report + live dashboard;
  generic, but the dashboard caches outside the repo (`XDG_CACHE_HOME`), conflicting with
  the project `.cache/` rule.

### REUSABLE for our manual scripts

- Xvfb self-wrap shape (`env -u WAYLAND_DISPLAY xvfb-run -a --server-args="-screen 0 WxHx24"`)
  and the WM-inside-Xvfb rationale (focus + window teardown on relaunch).
- Window-title matching + "X Y W H" bounds seam; logical→window-relative coordinates
  (our toast viewports need their own rects).
- launch/quit + PID-scoped snapshot cleanup + `kill_tree` + crashlog hook.
- Region crop + `compare -metric AE -fuzz` pixel assertions; failure artifact with region
  boxed red; results TSV + HTML + exit code; isolated config per launch.
- NOT reusable as-is: the driver's `xdotool`/`scrot`/`openbox` (absent here), Wayland/macOS/
  Windows drivers, kaleidotron viewport/zoom specifics, fixtures, dashboard cache location.

### APP-BOOT (verified by real runs, scratch dirs cleaned)

- `src/main.rs:164` — `Command::Shot` → `run_shot` (:184, synchronous capture, prints saved
  path, exit 0/1, `LOG_DRAIN_GRACE` 200 ms); no subcommand → `run_resident` (:189): instance
  guard (:321, exit 1 if lock held), `hotkeys::HotkeysController::start` (:338),
  `tray::TrayController::new` (:352), `gui::run` (:364), `park_without_gui` (:388) on GUI
  failure. `shot` is deliberately NOT instance-guarded (:40-42) so scripted captures work
  while the resident app runs.
- `src/gui.rs:1437-1464` — `eframe::run_native`, `ViewportBuilder` title
  `"virshot — Settings"`, transparent; eframe 0.37 features `["glow","default_fonts","x11","wayland"]`
  → winit picks **x11 when `WAYLAND_DISPLAY` is unset**. Toast viewport
  (`gui.rs:1063-1082`): `show_viewport_deferred`, title `"virshot"`, no decorations,
  transparent, mouse_passthrough, always_on_top; monitor placement uses
  `ViewportInfo::monitor_size` (`None` on Wayland, `gui.rs:77`).
- `src/hotkeys.rs:448-486` (linux) — `DISPLAY ":N"` requires socket `/tmp/.X11-unix/XN`
  (:473-476) → Unavailable if missing; no `DISPLAY` → Unavailable "DISPLAY is not set
  (Wayland-only host; hotkeys are X11-only)" (:483-484). Linux path is the `global-hotkey`
  X11 pump thread.
- `src/tray.rs:27-29,166-168` — ksni needs a D-Bus session bus; typed failure, app keeps running.
- xcap 0.9.8 picks wayland vs `xorg_capture` via `wayland_detect()`
  (`XDG_SESSION_TYPE=wayland` or `WAYLAND_DISPLAY` containing "wayland") → our resident app
  and `shot` need `WAYLAND_DISPLAY` unset to take the X11 path under Xvfb.

### VERIFIED RUN RESULTS

- **GUI under Xvfb works**: `timeout 20 env -u WAYLAND_DISPLAY xvfb-run -a
  --server-args="-screen 0 1280x1024x24" ./target/debug/virshot --config …` → log shows
  instance lock, "Hotkey registered: PrintScreen (full_screen)", "alt+PrintScreen (window)",
  rect reserved, "System tray ready", "Settings window opening". `xwininfo -root -tree` shows
  `0x400003 "virshot — Settings" … 720x520+0+0`. Only warnings: winit XSETTINGS, XRandR
  "display's 0mm in size", scale factor 1. No GUI failure.
- **`shot` under Xvfb is flaky**: `Capture failed: failed to capture monitor image: Connection
  error, possible I/O error` (xcb ConnError) in ~6 of 8 runs; successful runs produced real
  PNGs at exactly the screen size. Always warns `foreground window unavailable:
  _NET_CLIENT_LIST_STACKING not supported; using primary monitor` → window-mode capture needs a WM.
- ImageMagick `import -window root` on the same Xvfb succeeded (xcap's GetImage failed there).

### CONSTRAINTS / GAPS

- Input automation (click/type/key) needs `xdotool` (absent) → new native package, needs user
  approval; capture/bounds work today with ImageMagick + `xwininfo`.
- Hotkey E2E/manual check needs an X11 socket, which Xvfb provides — impossible on the bare host.
- Tray: D-Bus session bus exists and tray built OK under Xvfb; whether a StatusNotifier host
  renders it is unverified.
- Unknowns: root cause of intermittent xcap GetImage failure; whether glow GL actually paints
  under Xvfb (window maps, pixels unverified); toast-viewport placement without WM/monitor info;
  single-instance guard vs parallel Xvfb runs; how to terminate the resident loop from a script
  (tray Exit is the only exit; GUI close hides instead).
- No QA/e2e script exists today (`scripts/` = check.sh, check_full.sh, cratedoc.sh).
  `tests/cli_headless.rs` never runs bare `virshot`; the ignored test
  `cli_shot_full_screen_success_prints_saved_path_and_exits_zero` (:286-290) says the success
  path needs a desktop session and stays out of the automated gate (:282-285).
- Scratch precedent: `./.tmp/capture-harness/` (throwaway xcap probe crate from Stage 5).

## R2b — Re-verify after user installed `xvfb`, `xdotool`, `scrot`, `grim`, `weston`

(user installed 2026-11-03; scratch dirs `./.tmp/reverify-x11`, `./.tmp/reverify-wayland`
created and removed)

### TOOLS — all present

- `xdotool` 3.20160805.1, `scrot` 1.10, `grim` 1.4.0+ds-2build2 (no `--version`; `-h` works),
  `weston` 13.0.0, `xvfb-run`/`Xvfb` (xvfb `2:21.1.12-1ubuntu1.9`), `import` (ImageMagick 6.9.12-98).
- Still **missing**: `openbox`, `wmctrl` → a window manager is a new package, needs user approval.

### KEY ENV FACT: Xvfb socket creation

- `/tmp/.X11-unix` does not exist, `/tmp` is user-owned; bare `Xvfb :9` starts but creates **no
  socket** (`_XSERVTransmkdir: ERROR: euid != 0`), so the app fails:
  `Hotkeys unavailable: ... X11 display :9 has no socket file (/tmp/.X11-unix/X9)` +
  `GUI failed: ... Failed to open connection to X server` + `virshot idle: running without GUI`.
- Fixes: `mkdir -p /tmp/.X11-unix` before bare Xvfb, **or** plain `xvfb-run -a` (works with no
  mkdir, verified twice) → prefer `xvfb-run -a`.
- Sandbox gives **each bash call a fresh `/tmp`**: Xvfb + app + capture tools must run inside
  **one** call/script; the display cannot survive between calls.

### X11-PROBE (works — this is the "see the app" path)

`xvfb-run -a --server-args="-screen 0 1280x1024x24"` with `env -u WAYLAND_DISPLAY` and scratch
`--config`/log dirs → GUI opens:
- `xwininfo -root -tree` → `0x400003 "virshot — Settings": ("" "virshot") 720x520+0+0`.
- `xdotool search --name "virshot"` → `4194307` (rc 0); `--name "virshot — Settings"` → **rc 1**
  (WM_CLASS match only → match on `"virshot"`).
- `xdotool getwindowgeometry` → Position 0,0 / 720x520; `getwindowname` → `"virshot — Settings"`;
  `getdisplaygeometry` → `1280 1024`.
- `scrot` → rc 0, PNG 1280x1024 8-bit sRGB (35856 B); `import -window root` → rc 0 (35801 B);
  `scrot` with app running → 36582 B, **non-blank, UI visible**.
- Log has warnings only (XSETTINGS, XRandR "0mm size", scale factor 1) — no GUI failure.
- Sandbox blocks the default save path: `~/Pictures/Screenshots` → "Read-only file system
  (os error 30)" → **`save_path` must point into the workspace (./.tmp/)**.
- Config gotcha: `output_dir` is ignored; correct shape is `[general]\nsave_path = "<dir>"`.

### INPUT without a WM — works

- `xdotool windowfocus --sync <W>` rc 0; `xdotool key --window <W> Print` rc 0;
  `xdotool key Print` rc 0. Log recorded a hotkey-triggered capture:
  "Hotkey registered: PrintScreen (full_screen) / alt+PrintScreen (window) / control+PrintScreen
  reserved", "Saved: …/out/FullScreen/26-11/26-11-03/2026-11-03_15-46-15.png" (PNG 1280x1024),
  "Toast shown on 1 monitor(s)". Window-mode falls back to full-screen:
  "foreground window unavailable: _NET_CLIENT_LIST_STACKING not supported" → window-scoped
  capture needs a WM.

### WAYLAND — dead end for "seeing" the app

- `weston --backend=headless --socket=wayland-1 --width=1280 --height=1024 --no-config
  --renderer=pixman` with `XDG_RUNTIME_DIR=./.tmp/...` **does run** (creates `wayland-1` + `.lock`);
  `--backend=headless-dark` accepted. `--backend=headless --shell=none` created **no socket**.
- **No screenshot path**: `grim` → "compositor doesn't support wlr-screencopy-unstable-v1" (rc 1)
  on both headless `wayland-1` **and** the live WSLg `wayland-0` (`/mnt/wslg/runtime-dir/wayland-0`
  is reachable but same error); `weston-screenshooter` → "Output capture error: unauthorized".
- App on wayland-1: "Hotkeys unavailable: DISPLAY is not set (Wayland-only host; hotkeys are
  X11-only)", tray ready, "Settings window opening", no GUI-failed line, but no client/surface
  lines in the weston log → GUI state unverifiable and uncapturable.
- `virshot shot` on Wayland → "Capture failed: failed to list monitors: Connection closed, error
  during parsing display string" (xcap is X11-only here; no portal).
- Live host `DISPLAY=:0` (WSLg) is **not reachable** from the sandbox: `scrot` → "Can't open X
  display … [:0]" rc 1.

### `virshot shot` reliability under Xvfb

- Tight loop, 8 runs → **3/8 OK**; 1 s spacing, 10 runs → **2/10 OK**. Failure text always
  "Capture failed: failed to capture monitor image: Connection error, possible I/O error" (rc 1,
  no file) → xcap ≈ 25–40 % reliable.
- `scrot` in the same Xvfb: **5/5 rc 0** → scrot is reliable, xcap (the app's own capture) is not.
- With no DISPLAY: "failed to list monitors: Connection closed, error during parsing display string".

### R2b constraints for the scripts

- One script = one bash call: Xvfb + app + capture tools inside it, kill Xvfb at the end.
- Prefer `xvfb-run -a`; for bare `Xvfb` pre-create `/tmp/.X11-unix`.
- `save_path` under `./.tmp/`; remove `config.toml.lock` (instance lock) between runs.
- No WM ⇒ no window-scoped capture; installing `openbox` needs user approval.
- Plan scripts on the **X11/Xvfb** path: launch + `xwininfo`/`xdotool` + `scrot` (+ `import`) +
  log tail. Wayland/grim/weston cannot capture here.
- Unknowns: root cause of xcap's intermittent GetImage "Connection error" (a retry loop is needed);
  whether the GUI truly maps on weston headless.

## R2c — Window-level observation under Xvfb **without a WM** (after user added `wmctrl`)

### WM facts

- `wmctrl` 1.07 present; `openbox` still absent. `xprop -root` shows only `_XKB_RULES_NAMES` —
  **no `_NET_*` properties at all**.
- `wmctrl -l` → rc 1 "Cannot get client list properties. (_NET_CLIENT_LIST or _WIN_CLIENT_LIST)";
  `wmctrl -m` → rc 1 "Cannot get window manager info properties". → `wmctrl` is useless without a
  WM (it is a control tool, not a WM).

### Window capture works without a WM

- App window id via `xdotool search --name virshot` → `4194307` (0x400003), geometry 720x520 at 0,0;
  `xwininfo -id` → Map State IsViewable, border 0, parent = root, **absolute == relative origin**
  (no reparenting/decorations) → client-area origin == window origin.
- `import -window <id>` → rc 0, 720x520 PNG 32076 B, 350 unique colors — **real egui settings UI**
  (verified by reading the image). `scrot -w <id>` → rc 0, 720x520, same content.
- `scrot -o` (root) → rc 0, 1280x1024 (35856 B); `import -window root` → rc 0, 1280x1024 (35801 B).
- `scrot -c` → rc 0 but captures 1280x1024 (no `_NET_ACTIVE_WINDOW`; silently full-screen) →
  do not rely on it for window evidence. `grim` → rc 1 (no `wlr-screencopy-unstable-v1`).
- `convert -crop` from a root shot equals the window capture (`compare` rc 0); after
  `xdotool windowmove <id> 120 90`, crop `720x520+120+90` == window capture (rc 0) →
  window-region cropping from a root shot is a valid substitute.

### App `--window` mode needs EWMH (real WM)

- CLI is `virshot shot [--window] [--out <path>] [--format png|jpg|webp]`.
- `shot --window` → rc 1, "Capture failed: failed to list windows: _NET_CLIENT_LIST_STACKING not
  supported", **no file** — identical even while the resident app window is mapped; **no
  fallback** to full-screen in window mode (`Error::NoActiveWindow` is never reached).
- `shot` (full-screen) → WARN "foreground window unavailable … using primary monitor" then
  INFO "Saved:" rc 0 (1280x1024, 66799 B, app window visible). FLAKY: back-to-back runs alternate
  rc 0 / rc 1 ("Capture failed: failed to capture monitor image: Connection error, possible I/O
  error") → **needs a retry loop**. With nothing mapped it saves a **blank** PNG (2 unique colors).

### xdotool verbs without a WM

| verb | rc | effect |
| --- | --- | --- |
| `windowfocus --sync <id>` | 0 | app alive |
| `windowraise <id>` | 0 | app alive, no stacking semantics |
| `windowmove <id> 120 90` | 0 | position honored (wrong arg order `windowmove 120 90 <id>` → rc 1 BadWindow) |
| `windowsize <id> 500 400` | 0 | geometry honored (capture becomes 500x400) |
| `windowminimize <id>` | 0 | no-op |
| `windowactivate --sync <id>` | 1 | "windowmanager claims not to support _NET_ACTIVE_WINDOW" |
| `set_window --overrideredirect` | 1 | "There are no windows in the stack" |
| `windowkill <id>` | 0 | app dies rc 1 "X connection to :99 broken"; **not graceful** — the
  instance `<config>.lock` stays with a stale pid (a later resident launch replaces it) |
| SIGTERM | — | clean exit within ~1 s, no SIGKILL needed; SIGKILL also leaves the lock file |

### R2c implications for `./scripts/`

- Drive window evidence with `xdotool search --name virshot` + `import -window <id>` (or
  `scrot -w`) + `convert -crop` from a root shot; keep the app resident and stop it with SIGTERM;
  never `windowkill` as a graceful stop; add a retry loop for xcap full-screen `shot`.
- A real WM would add `_NET_CLIENT_LIST(_STACKING)`, `_NET_SUPPORTING_WM_CHECK`,
  `_NET_ACTIVE_WINDOW` → `wmctrl -l/-m`, `windowactivate`, `scrot -c`, and the app's own
  **window-mode capture**; plus reparenting/decorations, stacking, minimize, and WM_DELETE_WINDOW
  close semantics (today the window X never exits — tray Exit is the only exit path).
- `weston --backend=headless` cannot serve X11 here (`fatal: failed to add socket: Read-only file
  system`, `/run/user/1000` read-only) and is Wayland-only → grim stays unusable.

## R3 — Memory profiling workflow (flamegraph-rs/flamegraph)

### TOOL facts (main branch README + `src/bin/cargo-flamegraph.rs` / `src/lib.rs`)

- Install: `cargo install flamegraph` → binaries `flamegraph` and `cargo-flamegraph`.
- Forms: `cargo flamegraph` (profiles `cargo run --release`), `flamegraph -o out.svg -- <bin> <args>`,
  `flamegraph --pid N`, `cargo flamegraph --bin=X -- <args>`, `--dev`, `--unit-test`, custom perf
  via `-c "record -e ... -c 100 --call-graph lbr -g"`.
- **Memory profiling: there is NO `--memory` flag in current main** (verified `Options` struct).
  Memory events only via `-c "record -e mem:*:*:*"` (Intel PEBS required). `--palette mem` is
  coloring only. Older versions had `--memory`; current does not. Default recorder:
  `perf record -F 997 --call-graph dwarf,64000 -g -o perf.data`.
- Prereqs (README, quoted): Linux relies on `perf` (Ubuntu `linux-tools-*`, Debian `linux-perf`);
  "If you're using lld (default since Rust 1.90.0) or mold on Linux, you must use the
  `--no-rosegment` flag"; perf without root: `echo -1 | sudo tee /proc/sys/kernel/perf_event_paranoid`;
  `--root` runs via sudo; release quality needs `[profile.release] debug = true` or
  `CARGO_PROFILE_RELEASE_DEBUG=true`; `perf.data` + `flamegraph.svg` are written to CWD.
- Windows: native support via `blondie`, or `dtrace` — CPU-time sampling only; memory sampling is
  Linux-perf-specific.

### ENVIRONMENT (exact outputs)

- `which perf` → `/usr/lib/linux-tools/6.18.54.1-microsoft-standard-WSL2/bin/perf`;
  `perf --version` → perf 6.18.54.1. Host is **WSL2** (`uname -a` → DESKTOP-PLV9JA9,
  microsoft-standard-WSL2 kernel).
- `cat /proc/sys/kernel/perf_event_paranoid` → **2** (userspace sampling works without root, `:u`).
- `perf stat -e task-clock true` → works (exit 0). `perf record -o ./.tmp/test-perf.data -F 99
  --call-graph dwarf,64000 sleep 0.2` → works, wrote 0.005 MB.
- **Memory events do NOT work on this host**: `perf mem record` → "memory events not supported";
  `perf stat -e mem-loads,mem-stores` → "Unable to find event on a PMU of 'mem-loads'"
  (virtualized WSL2 PMU, no PEBS).
- `which flamegraph` → empty (**not installed**); install needs explicit user approval
  (AGENTS.md dependency rule). `./.cache/cargo` is writable and already holds installed binaries
  (`rusty-hook`), so `./cargo.sh install flamegraph` would be convention-compliant;
  crates.io API reachable with a User-Agent header (bare curl → 403).
- No alternative profiler present: `valgrind`, `heaptrack`, `samply`, `dhat`, `massif`, `heatpad`
  all not found.

### PROJECT facts

- `./scripts/` today: `check.sh` (check + test + clippy -D warnings), `check_full.sh`
  (+ `cargo fmt` / `fmt --check`), `cratedoc.sh` (rustdoc → markdown). No profiling script.
- Release build: no wrapper script — `./cargo.sh build --release` (artifacts exist:
  `target/release/virshot`, `target/x86_64-pc-windows-gnu/release/virshot.exe`).
- Profileable workload: `virshot shot` runs `run_capture` synchronously — the easy repeatable
  workload (capture → encode → save, looped N times). Bare `virshot` = resident daemon
  (instance guard, tray, hotkeys on main thread, eframe GUI or 1s park loop) — profiles idle
  ticks unless a hotkey triggers `capture::spawn_capture`.
- Gotchas: `[profile.release]` has `strip = true` + `lto = "thin"` → perf gets no symbols, so a
  profiling build needs `CARGO_PROFILE_RELEASE_DEBUG=true` and strip off; Linux dev host uses
  `lld` (Rust ≥ 1.90) → needs `-Clink-arg=-Wl,--no-rosegment`; `perf.data`/`flamegraph.svg` must
  land in `./.tmp/`.

### R3 constraints / gaps

- `flamegraph` is a new tool → user approval required before install.
- **True memory-access sampling (perf mem / PEBS) is impossible on this WSL2 kernel**, so the
  flamegraph workflow here is CPU-time sampling; allocation-level memory profiling would need a
  different tool (in-process heap profiler crate such as `dhat`, or `valgrind`/`heaptrack`) —
  each needing user approval.
- Windows-side profiling (blondie) is CPU sampling only and cannot run on this host.

## R3b — Re-verify after user built `perf` (6.18.g7d0a66e4bb90) and installed `valgrind-3.22.0`

### PERF (user-built at `/usr/local/bin/perf`, `perf -v` → 6.18.g7d0a66e4bb90)

- `perf_event_paranoid` = 2 → own-process CPU sampling works unprivileged.
- `perf list hw | grep -i mem` → rc 1 (no mem aliases); `mem-loads`/`mem-stores` still absent
  (rc 129 "Bad event name"). **New:** `perf list mem` now exposes the TMA `Mem:` group
  (`mem_inst_retired.all_loads` / `.all_stores` / `.any` / `.lock_loads` / `.split_loads`).
- `perf stat -e mem_inst_retired.all_loads true` → **rc 0**, counted 37056 → memory-event
  **counting works** with the new perf build.
- **Memory sampling still impossible**: `perf mem record -- true` → rc 255 "memory events not
  supported"; `perf record -e cpu/mem_inst_retired.all_loads/pp -- true` → rc 255 "PMU Hardware
  doesn't support sampling/overflow-interrupts. Try 'perf stat'" → WSL2 limitation persists;
  use `perf stat` counting, not `perf mem`.
- `perf record -o ./.tmp/…/p.data -F 99 --call-graph dwarf,64000 ./target/debug/virshot shot …`
  → perf side works (85512 B), `perf report --stdio` resolves symbols (warns about missing
  libunwind/libdw in the perf build), `perf script` rc 0. Caveat: headless `shot` runs ~0.228 s
  → ~1 sample at -F 99; CPU sampling needs a longer workload (resident daemon or looped shots).

### VALGRIND 3.22.0 (`/usr/bin/valgrind`)

- `--time-in-seconds` is **not** a massif option (rc 1). Valid options include
  `--massif-out-file` (default `massif.out.%p` lands in **CWD** → repo root; must pass
  `--massif-out-file=./.tmp/…`) and `--time-unit=ms`.
- Real massif run on headless path: `valgrind --tool=massif ./target/debug/virshot shot …` → rc 1
  (xcap failure, expected), wall 3.258 s vs 0.228 s native (**~14× slowdown**), `massif.out.5`
  240962 B; snapshots reach `mem_heap_B` 47142 (max 47301); heap tiny because capture fails before
  encode. `msprint` / `massif-visualizer` are **not installed** → report must be read from the text file.
- `valgrind --tool=memcheck ./target/debug/virshot --help` → rc 0; "total heap usage: 280 allocs,
  275 frees, 39,612 bytes allocated"; LEAK SUMMARY: definitely lost 0, still reachable 2698 B;
  "ERROR SUMMARY: 0 errors from 0 contexts".
- GUI under valgrind works **with correct nesting**: `valgrind …` wrapping the app directly
  (`valgrind … env -u WAYLAND_DISPLAY DISPLAY=:9 ./target/debug/virshot`); window found via
  `DISPLAY=:9 xdotool search --name virshot` within ~1 s, guest RSS ~150 MB. Nesting the other way
  (`valgrind` wrapping `xvfb-run`) profiles the **wrapper**, not the app (trace-children=no).
- **Blocking issue:** a resident guest never exits → forced `kill -TERM`/`-INT` produces **no
  massif/memcheck report** (`main.rs` has no signal handler). GUI-path reports need a natural exit;
  headless/looped paths are the reportable ones.
- Stale instance lock `<config>.lock` survives SIGTERM and its recorded pid can collide with a live
  pid → restarts fail ("another virshot instance is running (pid 4)") → workflow must
  `rm <config>.lock` after a forced kill.

### FLAMEGRAPH / post-processing status

- `flamegraph`, `cargo-flamegraph`, `inferno`, `hotspot`, `massif-visualizer`, `msprint`, `dhat` →
  all **absent**; `valgrind` and `perf` present. So perf data can be read via `perf report --stdio`
  / `perf script`, but **SVG rendering is blocked** until the user approves installing
  `flamegraph` (or `inferno`).
- `[profile.release]` (Cargo.toml:48-51): `opt-level = "z"`, `lto = "thin"`, `strip = true` →
  release binary has **no symbols**; profile the debug binary, or add an unstripped profiling
  profile (approval needed). `.cargo/config.toml` sets only the windows-gnu linker/subsystem →
  no Linux profiling rustflags (no `--no-rosegment`).

### R3b workflow shape (facts → plan input)

- CPU sampling: `perf record --call-graph dwarf,64000` on the resident daemon or looped
  `virshot shot`; text analysis via `perf report --stdio` / `perf script`; SVG needs approval.
- Heap: `valgrind --tool=massif --time-unit=ms --massif-out-file=./.tmp/…` on the headless/looped
  path (≈14× slower); memory-event counts via `perf stat -e mem_inst_retired.all_loads,…`.
- Leaks: `valgrind --tool=memcheck` on CLI paths (rc 0, 0 errors today).
- Never rely on `perf mem`; GUI-under-valgrind needs SIGTERM + `rm <config>.lock` cleanup and
  yields no report unless the guest exits naturally.

## R2d — Tray + real WM observation (user added `tint2` and `openbox`)

Recovered from the aborted research run's scratch evidence (`./.tmp/uiverify/`: per-case logs,
reports, panel screenshots, extracted tray icon). The tint2/openbox interaction loop was
**dropped by user decision 2026-11-03** ("пропустим этот момент и пойдем к реализации").

### openbox inside Xvfb → full EWMH

- `xvfb-run -a … bash -c 'openbox & virshot …'` works; openbox needs no config for defaults.
- `xprop -root` gains: `_NET_CLIENT_LIST`, `_NET_CLIENT_LIST_STACKING`, `_NET_SUPPORTING_WM_CHECK`
  (→ window `0x20011f` "Openbox"), `_NET_ACTIVE_WINDOW`, `_NET_CURRENT_DESKTOP`,
  `_NET_NUMBER_OF_DESKTOPS=4`, `_NET_WORKAREA`, `_NET_SUPPORTED`.
- `wmctrl -l` → rc 0: `0x00600003  0 DESKTOP-PLV9JA9 virshot — Settings`; `wmctrl -m` → rc 0,
  `Name: Openbox`, `PID: N/A`. `xdotool getactivewindow` → rc 0 (same id as search).
- Window becomes **reparented** into a frame: `xwininfo -id` absolute origin `280,259`,
  relative origin `1,20` (openbox titlebar), client 720x520. So window-id capture still works,
  but root-shot cropping must use the **absolute** origin.

### Window-mode capture works with a WM

- `virshot shot --window --out …` while the settings window is mapped → rc 0, log
  `INFO virshot::capture: Active window: virshot — Settings (720x520 at 280,259, primary monitor)`,
  `Saved: ./.tmp/uiverify/out/w.png`; PNG 720x520 37877 B (real UI), file name from the window
  title: `virshot — Settings-2026-11-03-14-17-58.png` (template uses the title).

### Tray is observable via D-Bus (not via pixels)

- Tools present: `dbus-run-session`, `dbus-daemon`, `busctl`, `gdbus`, `dbus-send`.
- Under `dbus-run-session --` the app logs `INFO virshot: System tray ready` and registers bus name
  `org.kde.StatusNotifierItem-<pid>-1` (visible in `busctl --user list`).
- `org.freedesktop.StatusNotifierWatcher` → **ServiceUnknown**: no watcher on this host;
  `tint2` links libxcb/libX11 only (no libdbus) and contains **no `statusnotifier` strings** →
  tint2's systray cannot render SNI items. Panel screenshot (`panel-t6d.png`, 1280x30 at y=994)
  shows tint2's own icon, Chrome icon, `desktop 1`, a **taskbar button** `virshot — Settings`,
  clock — no tray icon.
- SNI properties (via dbus): `Status=Active`, `Category=ApplicationStatus`, `Id='<pid>-1'`,
  `Title=''` (empty), `ToolTip` = `(32x32 icon pixmap, "virshot")`, `ItemIsMenu=0`;
  `IconPixmap` = 32x32 RGBA 4096 B → extractable to PNG (`icon.raw` → `convert` → `icon.png`,
  grey camera glyph, pixels 80/200 grey).
- Menu: `/MenuBar` `com.canonical.dbusmenu.GetLayout(0,-1)` → revision 0, 5 items:
  id 1 `Take screenshot`, id 2 separator, id 3 `Settings`, id 4 `Exit`; `AboutToShow(0)` → 0.
  `Event id=3` → settings window Map State `IsUnMapped` → `IsViewable`; `Event id=4` → app exits
  cleanly (`INFO virshot::gui: virshot exiting from tray menu`) and `<config>.lock` is removed.
- `org.kde.StatusNotifierItem.Activate()` → re-shows a hidden window (rc 0).

### Close/stop semantics with a WM

| action | result |
| --- | --- |
| `wmctrl -c virshot` / `windowclose` (WM_DELETE) | **hide only** — app alive, window `IsUnMapped`, lock stays |
| `xdotool windowclose <id>` (frame) | app exits **cleanly**, `<config>.lock` **removed** |
| SIGTERM | app gone, lock **stale** (must `rm`) |
| SIGINT | ignored, app keeps running |
| `xdotool windowkill <id>` | app gone (X broken), lock stale |
| restart after SIGTERM | succeeds (stale lock replaced) unless the recorded pid collides with a live pid |

### Hotkeys under openbox — unresolved, deliberately not chased

- App **alone** under Xvfb (no WM): `INFO Hotkey registered: PrintScreen (full_screen)` +
  `INFO Hotkey registered: alt+PrintScreen (window)` + `Hotkey reserved (rect area, out of MVP scope)`.
- With **openbox** running: `ERROR Hotkey not registered: PrintScreen (full_screen): HotKey already
  registered: HotKey { mods: Modifiers(0x0), key: PrintScreen, id: 117 }` (same for id 118).
  Mechanism from `global-hotkey-0.9.0/src/platform_impl/x11/mod.rs:114-186`: the message is either
  an X-level `Access` error on `grab_key` (another client already grabbed that keycode on the root
  window — e.g. a still-alive earlier virshot in the same display) or the in-process duplicate check
  (`line 174-184`). Root cause not pinned (research skipped).
- Consequence for scripts: assert the hotkey path in the **no-WM** configuration (registration
  succeeds there); a WM run may log registration errors, so do not assert `Hotkey registered`
  under openbox.

### R2d implications for `scripts/ui-verify.sh`

- Default shape: `xvfb-run -a` + `dbus-run-session --` + resident app (no WM) → window capture via
  `import -window <id>`, tray evidence via D-Bus (`StatusNotifierItem` name, `ToolTip`/`IconPixmap`,
  `/MenuBar` layout, `Activate`, menu `Event` for Settings/Exit), hotkey registration assertions.
- Optional `--wm` (openbox) adds EWMH: `wmctrl -l/-m`, `getactivewindow`, and the app's own
  `shot --window` mode; then crop from absolute origin; drop hotkey assertions.
- Tray icon **visibility** is asserted as D-Bus pixmap → PNG (compare against the app's rendered
  icon), not as a panel screenshot: tint2 cannot render SNI items on this host.

## R2d-update — hotkey-under-WM gap, as observed in Stage 2 (2026-11-03T20:19+03:00)

`./scripts/ui-verify.sh all` (no WM): `INFO Hotkey registered: PrintScreen (full_screen)` +
`alt+PrintScreen (window)`, hotkey capture PASS (`Saved: … out/FullScreen/…png`, toast logged).
`./scripts/ui-verify.sh all --wm` (openbox): the same startup logs
`ERROR virshot: Hotkey not registered: PrintScreen (full_screen): HotKey already registered …`
→ hotkey assertions and hotkey capture are **skipped** in WM mode (script header documents it).
Attribution: the WM holds the PrintScreen grab on the root window, so `grab_key` returns an X
`Access` error which `global-hotkey` maps to `AlreadyRegistered` (`x11/mod.rs:157-164`). Kept as a
workflow constraint, not chased further (user decision: skip the tint2/openbox loop).
