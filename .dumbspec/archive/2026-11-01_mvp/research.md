# 2026-11-01_mvp: research

Repo: `/home/apkawa/code/vibe-shot`, package `virshot`, branch `2026-11-01_mvp`. HEAD `1d2b530 chore: rename project to VIRShot and add README`. Spec: `docs/spec.md` (241 lines, Russian, dated 2026-11-01 after discussion rounds 1–4). Read-only research pass (subagent), facts only.

## Spec MVP items

Spec §2 "Входит в MVP" (docs/spec.md:17-28) verbatim list, with the section that details each:

1. **Gamepad direct signals** — spec.md:19 "Прямое чтение сигналов геймпада (без эмуляции клавиатуры)." → §4.1 (:62-78).
   - §4.1:64-65 "Фоновый поток опрашивает подключённые геймпады через `gilrs` (plug-and-play; при отключении поток не падает, ждёт переподключения)."
   - §4.1:66-67 "События подключения/отключения геймпада логируются на уровне INFO; тосты для них не показываются."
   - §4.1:68-74 every action ("весь экран", "окно", "область" [reserved]) has **two bind fields: Основное и Дополнительное**; each field holds **either a keyboard combo (key + arbitrary Ctrl/Alt/Shift modifiers, e.g. `Alt+PrintScreen`) or one gamepad button**; multi-button gamepad combos **not supported**; fields fill independently; **action fires on any filled field (OR)**.
   - §4.1:75-76 binding UI: click on field → capture mode "нажмите кнопку" (not a dropdown); the tab shows the list of connected gamepads (names from `gilrs`).
   - §4.1:77 "**Debounce** одного действия ~250 мс (повторный триггер короче интервала игнорируется)."
   - §4.1:78 "Ошибка регистрации хоткея (конфlict/недоступно) — статус в UI + лог, без падения."
2. **Screen capture** — spec.md:20-21 "Захват экрана: **весь экран** (монитор с foreground-окном) и **окно** (захват содержимого окна независимо от z-порядка)." → §4.2 (:80-105).
   - §4.2:82-83 target chosen at trigger time by foreground window; the app's own windows (settings/overlay) excluded from analysis.
   - §4.2:84-85 full screen = monitor containing the foreground window; if foreground undetermined → fallback to primary monitor.
   - §4.2:86-89 window = foreground window content regardless of occlusion (Windows: PrintWindow); no active window (desktop visible) → **error** "no active window" in toast + log, file not saved; capture failure → also error in toast + log.
   - §4.2:90-91 formats `png` (default, lossless), `jpg` (quality, default 90), `webp` (lossy, quality, default 90); format is a **global setting**.
   - §4.2:92-97 filename template tokens `%Y %y %mo %d %H %M %S %iso %t %n %ext`; `%iso` = `YYYY-MM-DD_HH-mm-ss`, **local** time; `%t` = window title in window template, literal `fullscreen` in full-screen template; `%n` = collision counter in target folder determined by **scanning the target folder on disk at save time (no state file)**; `%ext` = chosen format extension.
   - §4.2:98-99 sanitization: invalid chars in `%t` → `-`, empty title → `untitled`, truncate to ~60 chars; template segments sanitized.
   - §4.2:100-101 template validation: relative path from `save_path` only; absolute paths and `..` forbidden (error in log + toast).
   - §4.2:102-103 collisions: overwrite forbidden; suffix `-2`, `-3`… before extension (or `%n` value if present).
   - §4.2:104 missing subfolders of the template are created automatically.
   - §4.2:105 "Сохранение выполняется в отдельном потоке (без микро-фризов в игре)."
3. **Overlay toast** — spec.md:22 "Оверлей-уведомление (тост): успех и ошибка; ошибки видны всегда." → §4.3 (:107-118).
   - :109 one toast window, **new toast replaces current** (no stack).
   - :110-111 transparent, no decorations, always-on-top, **mouse passthrough** (clicks go to the game); implementation egui multi-viewport vs native window — **⏳ research**.
   - :112-113 fade in/out fixed ~250 ms; hold = `duration_ms` (default 2000 ms); errors held ~3000 ms.
   - :114-115 position: 4 corners (`position = TopLeft|TopRight|BottomLeft|BottomRight`), fixed offset ~16 px.
   - :116 setting `on_all_monitors` (default `true` — duplicated on all monitors).
   - :117-118 content (English): success `Saved: <file name>` (full path in log); error = error text; errors shown **even when `overlay.enabled = false`**.
4. **Settings window (GUI)** — spec.md:23 "Окно настроек (3 вкладки, кнопка Apply)." → §4.4 (:120-139).
   - :122 bordered window, default size ~720×520; position/size not persisted.
   - :123 closing via X → program hides to tray (does not exit).
   - :124-125 **Apply button** writes `config.toml` and applies changes live; closing without Apply discards unsaved edits.
   - :126 UI language English. Tab 1 "General" (:127-131): `save_path` + Browse; full-screen template default `FullScreen/%y-%mo/%y-%mo-%d/%iso.%ext`; window template default `%t/%y-%mo/%y-%mo-%d/%t-%iso.%ext`; file format; `jpeg_quality`; `webp_quality`.
   - Tab 2 "Hotkeys" (:132-137): 3 columns Action | Primary | Secondary; actions `Full screen` (PrintScreen), `Window` (Alt+PrintScreen), `Rect area` (Ctrl+PrintScreen — slot reserved, inactive in MVP); each field = keyboard combo or one gamepad button (bind-by-press); list of connected gamepads; registration-error statuses in UI + log.
   - Tab 3 "Overlay" (:138-139): `enabled`, screen corner, `duration_ms`, `on_all_monitors`.
5. **System tray** — spec.md:24 "Работа в системном трее (скрытый запуск, контекстное меню)." → §4.5 (:141-147): built-in icon (simple camera glyph; custom later); left click → open settings; right click menu `Take screenshot` (full-screen capture), `Settings`, `Exit`; exit only from tray menu; `tray-icon` event-loop integration **⏳ research**.
6. **Config file** — spec.md:25 "Конфиг `config.toml` по конвенциям OS, авто-создание дефолта при первом запуске." → §7 (:169-208): Windows `%APPDATA%\virshot\config.toml`; Linux `~/.config/virshot/config.toml`; overridden by `--config`; default config created on first run; unparsable/broken config → error in log + run on defaults (:171-173). Draft schema (:178-208): `[general] save_path, file_full_screen, file_window, format, jpeg_quality, webp_quality`; `[controls.full_screen] key/mods/pad`, `[controls.window]`, `[controls.rect]` (reserved, inactive); `[overlay] enabled, position, duration_ms, on_all_monitors`. Schema marked draft — final **⏳ research** (:175-176).
7. **Filename template tokens** — spec.md:26 "Шаблон пути файла с токенами: `%Y %y %mo %d %H %M %S %iso %t %n %ext`." (detail §4.2:92-97).
8. **Headless subcommand** — spec.md:27 "Headless-подкоманда `virshot shot` для тестов." → §8 (:210-225): flags `--config`, `--log`, `--log-level=DEBUG|INFO|ERROR`, `--version`, `--help`; subcommand `virshot shot [--window] [--out <path>] [--format png|jpg|webp]`; no GUI/tray/overlay; config is the default source; `--out`/`--format` override; prints saved path to stdout; exit 0 success, 1 error (:220-221). Logs: default `virshot.log` next to config; `--log` overrides; `--log-level` default `INFO`; no rotation; successful saves and gamepad plug/unplug are INFO (:223-225).
9. **Single-instance guard** — spec.md:28 "Single-instance guard." (no further detail anywhere in the spec).

**Not in MVP (future)** — spec.md:30-40: `rect` area capture (slots reserved in config/UI, no implementation), video recording / Replay Buffer, built-in image editor, exclusive full screen (documented limitation §9), autostart at login, custom tray icon (MVP uses built-in), i18n (MVP UI English only; config keys and CLI English; docs/README Russian), macOS out of scope.

**Non-functional (§5:149-155):** binary < 10 МБ (release `strip`, `lto`, `opt-level="z"`); RAM < 30 МБ in background (no capture); CPU < 1% at idle; no network requests / full privacy; fault tolerance — any error (no folder access, gamepad dropped, failed capture) logged + toast, no crash.

**Threading (§6:157-167):** main thread = eframe loop (settings window + overlay toast, overlay driven by a timer on the same thread) + tray events; input thread = `gilrs` loop → command via channel to main; capture thread = receives command, captures monitor/window, saves file. Keyboard system-wide hook thread/integration depends on crate choice — **⏳ research**.

**Delivery (§10:236-241):** unit tests = path template, sanitization, validation, config parsing/defaults; integration in WSL = headless `virshot shot` real capture/file write; Windows acceptance manual with checklist in chat (not in repo); MVP delivery = portable zip (`virshot.exe` + README), no installer, no auto-update.

**Platform targets (§3:56-58):** development in WSL — Linux build compiles and core functions run (capture/save/config/CLI); GUI and hotkeys on Linux best-effort per environment; final acceptance manually on Windows. Windows cross-build target `x86_64-pc-windows-gnu` (CONTRIBUTING.md:16-24, README.md:11-14).

## PoC coverage table

PoC is a single file: `src/main.rs` (219 lines) — inline modules `helpers` (:6-20), `events` (:22-66), functions `on_button_pressed` (:72-78), `take_screenshot` (:84-105), `gamepad_name` (:109-114), `main` (:116-174), `#[cfg(test)] mod tests` (:176-219). No other `src/` files, **no `tests/` directory** (only `docs/spec.md`, `scripts/cratedoc.sh`, `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`, `.rusty-hook.toml`, `cargo.sh`, `rust-toolchain.toml`, `README.md`, `CONTRIBUTING.md`, `AGENTS.md`, `.dumbspec/`).

Actual PoC flow today: `main()` inits `gilrs::Gilrs::new()` (:120-126, exits 1 on init failure), prints connected pads (:136-145), loops `gilrs.next_event()` with `sleep(10ms)` (:147-173); on `ButtonPressed` it prints `<pad>: <button> pressed` (:165) and calls `on_button_pressed` (:166) which spawns a thread (:74) that calls `take_screenshot(&dir)` (:84): `create_dir_all` → `xcap::Monitor::all()` → pick `is_primary()` else first (:89-94) → `monitor.capture_image()` (:96-98) → `image.save(dir.join("screenshot_%Y%m%d_%H%M%S.png"))` (:100-101). Save dir is hardcoded `./.tmp/screenshots` (:12-14). Errors returned as `Err(String)` (:84) and printed to stderr (:76). Success printed as `Saved: <path>` to stdout (:75). 7 unit tests: `events::format_press` ×4 (:34,46,54,61) + helpers ×3 (:182,203,213).

Declared deps unused in code (`rg` count 0 in src/main.rs): `eframe` (Cargo.toml:11), `egui` (:18), `clap` (:16), `tray-icon` (:17), `serde` (:12), `toml` (:13), `tracing` (:14), `tracing-subscriber` (:15), `thiserror` (:10). Used: `gilrs` (:7), `xcap` (:8), `chrono` (:9).

| Spec MVP item | PoC status | Evidence |
|---|---|---|
| Gamepad direct signals (spec.md:19, §4.1:62-78) | partial | polling loop + connect/disconnect + ButtonPressed src/main.rs:147-173; no action mapping, no per-action binding |
| Two bind fields, key combo OR pad button, OR-fire (§4.1:68-74) | missing | no binding model in repo; grep for `mods`/`bind` finds nothing |
| Bind-by-press UI + connected-pad list (§4.1:75-76) | missing | no GUI; pad names printed to console only src/main.rs:136-145 |
| Debounce ~250 ms (§4.1:77) | missing | comment src/main.rs:71 "POC: no debounce, no filename collision handling"; every press triggers (:163-167) |
| Hotkey registration error → UI status + log, no crash (§4.1:78) | missing | no hotkey crate at all |
| Global keyboard hotkeys (§3:49-50, §6:166-167) | missing | no crate in Cargo.toml; AGENTS.md marks it "TODO: research" |
| Capture full screen of foreground-window monitor (§4.2:84-85) | partial | primary-or-first monitor only src/main.rs:89-94; no foreground-window logic |
| Capture window (§4.2:86-89) | missing | `xcap::Window` never used (API exists in `.cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/xcap-0.9.8/src/window.rs:18,42,46,78,84`); no "no active window" error |
| Formats png/jpg/webp + qualities (§4.2:90-91) | partial (png only) | `image.save()` src/main.rs:101; locked `image` 0.25.10 pulls only `png` + `tiff` (Cargo.lock:1996-2006); no jpeg/webp encoders; xcap feature `image = ["image/default"]` (xcap-0.9.8/Cargo.toml:37) would add codecs |
| Filename template tokens (§4.2:92-97, spec.md:26) | partial | fixed literal `screenshot_%Y%m%d_%H%M%S.png` src/main.rs:17-19; no `%iso/%t/%n/%ext`, no folder segments |
| Sanitization (§4.2:98-99) | missing | no sanitizer code |
| Template validation (relative only, no `..`) (§4.2:100-101) | missing | none |
| Collision suffix `-2`, `-3`, `%n` folder scan (§4.2:102-103) | missing | comment src/main.rs:71 "same second overwrites"; no scan |
| Auto-create template subfolders (§4.2:104) | partial | `create_dir_all(dir)` src/main.rs:85-87 exists but dir is a fixed constant, no template paths |
| Save on separate thread (§4.2:105, §6:164) | done (shape) | `std::thread::spawn` src/main.rs:74; no channel/command plumbing to UI thread |
| Overlay toast (§4.3, spec.md:22) | missing | no eframe/egui code; stdout line `Saved: {}` src/main.rs:75 is the nearest analogue |
| Settings window 3 tabs + Apply (§4.4, spec.md:23) | missing | no GUI code |
| Tray + menu + hidden start (§4.5, spec.md:24) | missing | `tray-icon` declared Cargo.toml:17, never used |
| config.toml per-OS path + default on first run (§7, spec.md:25) | missing | no config module; save dir hardcoded src/main.rs:12-14 |
| CLI flags `--config/--log/--log-level/--version/--help` (§8:212) | missing | no `clap` usage; binary runs the gamepad loop unconditionally src/main.rs:147-173 |
| `virshot shot [--window] [--out] [--format]`, stdout path, exit 0/1 (§8:214-221) | missing | no subcommands |
| Logging `virshot.log` file appender, INFO/DEBUG/ERROR (§8:223-225, §3:53) | missing | `println!`/`eprintln!` only src/main.rs:75-76,123,128,137,139,152,160,165 |
| Gamepad plug/unplug logged INFO (§4.1:66-67, §8:224-225) | partial | console `println!("{name} connected/disconnected")` src/main.rs:150-162, not `tracing` INFO |
| Single-instance guard (spec.md:28) | missing | nothing in repo |
| Own windows excluded from foreground analysis (§4.2:82-83) | missing | no GUI windows exist yet |
| Fault tolerance: log + toast, no crash (§5:154-155) | partial | errors return `Err(String)` and print stderr, no panic src/main.rs:84-104; no toast, no log file |
| Binary < 10 MB (§5:151) | done | `target/x86_64-pc-windows-gnu/release/virshot.exe` = 915968 bytes (~0.87 MB), built Nov 1 15:56; profile `opt-level="z", lto="thin", strip=true` Cargo.toml:23-26 |
| RAM < 30 MB / CPU < 1% idle (§5:152) | unmeasured | no measurement artifacts in repo |
| No network requests (§5:153) | done | no network crate/deps |
| Unit tests for template/sanitization/validation/config (§10:238) | partial | 7 tests src/main.rs:34-64,182-218 cover only press formatting + filename/dir |
| E2E headless `virshot shot` writing a real file (§10:239, AGENTS.md `tests/`) | missing | no `tests/` directory |
| Windows manual acceptance + portable zip delivery (§10:240-241) | not started | no zip/README delivery artifacts |

## Fixed constraints

- **Build targets**: Linux native + cross Windows GNU. `rust-toolchain.toml:1-2` `channel = "stable"`; `Cargo.toml:4` `edition = "2024"`; spec §3:44 "Rust (latest stable, rust version 1.99.1)". Windows linker pinned: `.cargo/config.toml:1-2` `[target.x86_64-pc-windows-gnu] linker = "x86_64-w64-mingw32-gcc"`.
- **`./cargo.sh` wrapper** (`cargo.sh:1-7`): sets `CARGO_HOME="$here/.cache/cargo"` and execs cargo; global `~/.cargo` is read-only in the sandbox, so bare `cargo` fails (CONTRIBUTING.md:39-46).
- **Native packages (installed now)**: `mingw-w64 11.0.1-3build1` + `/usr/bin/x86_64-w64-mingw32-gcc`; Linux dev packages per CONTRIBUTING.md:12-14 are installed: `libudev-dev`, `libwayland-dev`, `libpipewire-0.3-dev`, `libclang-dev`, `libegl1-mesa-dev`, `libgbm-dev`, `pkg-config` (verified via dpkg). Windows cross-build needs only MinGW.
- **Pre-commit gate**: `.rusty-hook.toml:2` `pre-commit = "./cargo.sh check && ./cargo.sh test && ./cargo.sh fmt && git add -u . && ./cargo.sh clippy -- -D warnings"`, `[logging] verbose = true`. All 20 rusty-hook hooks installed in `.git/hooks/`, no `core.hooksPath`.
- **rusty-hook quirk**: the installed hook script requires the `rusty-hook` CLI on PATH; `command -v rusty-hook` → not found. CLI exists only in repo cache: `.cache/cargo/bin/rusty-hook`. Auto-install in `.git/hooks/cli.sh:14-20` runs `cargo install rusty-hook` targeting the read-only global `~/.cargo` → in the sandbox the hook prints "rusty-hook is not installed... skipping" and exits 0: the gate **silently skips** unless PATH includes `.cache/cargo/bin` (or `RUSTY_HOOK_SKIP_AUTO_INSTALL` is set). Explicit verification: `./cargo.sh check && ./cargo.sh test && ./cargo.sh fmt && ./cargo.sh clippy -- -D warnings`.
- **Crate API helper**: `scripts/cratedoc.sh:3-14` — `./scripts/cratedoc.sh <crate>::<item> [--all] [--full]`; repo-local CARGO_HOME; HTML→markdown. AGENTS.md: "Verify by building — not by memory." Scratch output goes to `.tmp/`.
- **`unsafe` forbidden**: AGENTS.md "Safety" — `#![forbid(unsafe_code)]` at crate root. **Currently absent**: `rg 'forbid|unsafe' src/` returns nothing; the attribute must be added.
- **Code structure**: AGENTS.md "One module = one file"; `src/main.rs` only wires CLI entry, event loop, module wiring; module tests in that file's `#[cfg(test)] mod tests` or sibling `<module>.tests`-style file. PoC has three inline modules in one file → MVP must split files.
- **Testing conventions**: unit tests TDD, pure logic only; E2E tests no-TDD regression in `tests/` running the built binary headless (`virshot shot`); tests pass on both targets: `cargo test` (linux) and `cargo test --no-run --target x86_64-pc-windows-gnu`. Test naming `<unit>_<scenario>_<expectation>`.
- **Formatting**: `cargo fmt` owns whitespace; commit messages Conventional Commits.
- **Dependency policy**: fixed list in AGENTS.md — `eframe`+`egui` (multi-viewport), `gilrs` `default-features = false, features = ["xinput"]`, `xcap`, `tray-icon` `default-features = false, features = ["ksni"]`, `serde`+`toml`, `thiserror`, `tracing`+`tracing-subscriber`, `clap`. New deps (crate, native package, toolchain target) require explicit user confirmation. No global-hotkey crate and no OS-path crate in the list → those need approval.
- **Error/threading practices**: typed per-module error enums via `thiserror`; no `unwrap()`/`expect()` outside tests/startup; app loop logs and keeps running; inject `DateTime` into filename/time producers; `pub(crate)` by default; no glob imports; avoid `.clone()` in hot paths; capture on spawned thread via owned values/channels, not shared `Mutex`.
- **Workspace hygiene**: temp/scratch only in `./.tmp/`, caches only in `./.cache/`, both git-ignored along with `target/`.
- **Scope guard**: MVP scope is exactly `docs/spec.md`.
- **Artifact names**: package/binary renamed to `virshot` (Cargo.toml:2); stale `target/x86_64-pc-windows-gnu/release/vibe-shot.exe` remains from pre-rename build.

## Ambiguities/risks

Spec-flagged open items (all marked **⏳ (ожидает research)** — docs/spec.md:4-6 preamble):
- Overlay implementation: transparent click-through toast via egui multi-viewport vs native window (spec.md:46, :111). Facts: egui 0.37 has `ViewportBuilder::with_transparent` (`egui-0.37.0/src/viewport.rs:430`), `with_mouse_passthrough` (:678), `Context::show_viewport_deferred` (:4608) / `show_viewport_immediate` (:4662); winit is in Cargo.lock (:5196).
- Global keyboard hotkey crate choice (spec.md:49-50, :166-176): system-wide hook needed so PrintScreen works while a game has focus; no crate in the dependency list; key notation for config strings unspecified (spec.md:175-176: final schema depends on the hotkey crate).
- `tray-icon` integration with the winit/eframe event loop (spec.md:51, :147). Facts: tray-icon 0.26.1 default = `libappindicator`; `ksni` = pure-Rust SNI/D-Bus (`tray-icon-0.26.1/Cargo.toml:34-42`); `compile_error!` if neither backend enabled on Linux (`src/lib.rs:130-138`). Linux SNI needs a running D-Bus session — WSLg feasibility open. Event bridging via `MenuEvent::receiver()`/`set_event_handler` (`src/lib.rs:83-120`).
- Final config schema and threading model refinement (spec.md:159, :175-176).
- Linux best-effort details for GUI/hotkeys/tray on X11/Wayland/WSLg (spec.md:231-232).
- Win11: PrintScreen may be intercepted by Snipping Tool — behavior/handling unresolved (spec.md:233-234).
- Exclusive full screen unsupported: capture may be black, overlay invisible; recommendation Borderless Windowed, no workaround (spec.md:229-230).

Additional repo-derived risks (facts):
- **Config path conventions** (`%APPDATA%\virshot\config.toml`, `~/.config/virshot/config.toml`, spec.md:171): no path-resolution dependency in Cargo.toml; `dirs` appears in Cargo.lock only as a `libappindicator` feature dep of tray-icon → OS dirs likely need a new crate or manual env handling (dependency-policy decision).
- **jpg/webp encoders absent today**: locked `image` 0.25.10 pulls only `png` + `tiff` (Cargo.lock:1996-2006); enabling xcap's `image = ["image/default"]` feature (xcap-0.9.8/Cargo.toml:37) pulls new codec crates — dependency approval territory.
- **`%mo` token undefined**: spec lists `%Y %y %mo %d %H %M %S %iso %t %n %ext` (spec.md:26, :92) but defines only `%iso`, `%t`, `%n`, `%ext`; `%mo` semantics (month segment, e.g. `2026-11`) inferred from defaults `FullScreen/%y-%mo/...` (spec.md:129-130, :181-182).
- **Gamepad button naming**: spec example `pad = "Share"` (spec.md:191) has no `gilrs::Button` variant; enum is `South, East, North, West, C, Z, LeftTrigger, LeftTrigger2, RightTrigger, RightTrigger2, Select, Start, Mode, LeftThumb, RightThumb, DPadUp/Down/Left/Right, Unknown` (`gilrs-0.11.2/src/ev/mod.rs:117-144`). Config representation of a captured button unspecified.
- **gilrs feature gating on Linux**: Linux backend is `cfg(target_os = "linux")` (`gilrs-core-0.6.8/src/platform/mod.rs:24-25`), independent of `xinput`/`wgi`, but `compile_error!` fires if neither feature enabled (`platform/mod.rs:32-35`) → `features = ["xinput"]` must stay for Linux builds too. On this host no pads detected in PoC runs (`.tmp/linux-loop-run.log`), Linux gamepad acceptance unverified.
- **xcap Linux chain**: X11/Wayland/PipeWire backends pulled unconditionally; Linux capture depends on the dev packages above; Wayland capture works only under a running compositor (WSLg).
- **Size/RAM budgets vs eframe defaults**: eframe default features include `accesskit, clipboard, default_fonts, links, system_fonts, wayland, web_screen_reader, wgpu, winit/default, x11` (`eframe-0.37.0/Cargo.toml:61-72`); budgets (spec §5) unmeasured against GUI stack in Cargo.lock.
- **PoC save dir conflicts with MVP semantics**: screenshots go to git-ignored `./.tmp/screenshots` (src/main.rs:12-14), not spec's `save_path` (spec.md:128, :180).
- **Locked transitive duplicates**: Cargo.lock contains `thiserror 1.0.69` (:4071) and `2.0.21` (:4080), `toml 0.5.11` (:4178) and `1.1.8+spec-1.1.0` (:4187).

## Gamepad Share / non-standard buttons research (spec v2)

Full report: `.tmp/gilrs-share-button-investigation.md` (source-verified against vendored gilrs 0.11.2 / gilrs-core 0.6.8).

- **No DirectInput features in gilrs 0.11.2**: crate features are only `default=["wgi"]`, `serde-serialize`, `wgi`, `xinput` (`gilrs-0.11.2/Cargo.toml`); earlier AGENTS.md/spec note "XInput / DirectInput" was wrong.
- **`xinput` + `wgi` mutually exclusive**: `compile_error!` in `gilrs-core-0.6.8/src/platform/mod.rs:38-39`, not target-gated → both features break Linux too; exactly one backend must be chosen on both targets.
- **XInput hides Share**: native event codes map through a closed 27-code table (`gilrs-core .../xinput/native_ev_codes.rs:530-561`); codes outside it are dropped, `Button::Unknown` is unreachable on Windows under xinput → Share/guide/extra buttons invisible.
- **WGI paths**: `FromGameController` (Gamepad API) yields the fixed canonical 14-button map — pads classified as Gamepad (Xbox Series) expose only canonical buttons; the Raw path (`ButtonPressed(Button::Unknown, code)`, `code.into_u32()` = raw button index) fires only when the Gamepad path fails. Raw indices are stable only per HardwareVendorId/HardwareProductId. WGI `RawGameController.GetButtonLabel` exposes Options=29/Share=30 but gilrs never calls it.
- **Compile experiments** (`.tmp/exp_*.txt`): xinput+wgi → exit 101 on linux and windows-gnu; wgi-only → exit 0 on both targets. Windows `windows` crate is not built under `wgi` on Linux.
- **Runtime remap exists** (`gilrs::Mapping::insert_btn`, `Gilrs::set_mapping`) but `Button::Unknown` is rejected as a mapping key.
- **Decision (user, spec v2)**: backend `wgi` + numeric notation `unknown:<N>` (raw index) for non-standard buttons; quick PoC logs `code.into_u32()` per press for hardware verification. Risks to verify on Windows hardware: Xbox Game Bar consumes Share globally (source-confirmed only as platform behavior, not by gilrs code); Gamepad-classified pads may hide Share; earlier AGENTS.md claim "WGI delivers no events to console/tray process" contradicted by SDL's Win32 WGI driver precedent — PoC settles it.

## Gamepad v3: custom Windows backend (settled 2026-11-01)
- Hardware result (user): generic HID pad vendor=1118 product=2835 via gilrs `wgi` — connect/disconnect arrive, zero press/axis events, no `Dropped`. Root cause: gilrs-core gates updates on `old.time()==new.time()` (`windows_wgi/gamepad.rs:180-208`); generic HID pads report a constant timestamp via `GetCurrentReading` → diffs never computed. Admin console does not change this.
- Fix (user-approved): own `gamepad` module on the `windows` crate — `RawGameController` enumeration + ~8 ms polling thread, button-STATE diff (ignore timestamps), names via `GetButtonLabel`; Linux keeps `gilrs`.
- **v4 (2026-11-02): gamepad deferred out of MVP (user decision).** User keeps a working gamepad PoC outside the MVP flow; all gamepad code/deps (`gamepad.rs`, `events.rs`, gilrs wiring, `gilrs` + target-scoped `windows` deps) removed from the repo. Future integration = polling loop over HID via `gameinput.dll` (`RawGameController`) on Windows + `gilrs` on Linux, plan Stage 7 DEFERRED; deps re-added with user approval when it lands. Root-cause finding above stands.

## Global hotkeys: global-hotkey v0.9.0 (settled 2026-11-03, Stage 6, commit bf2a427)
- Crate chosen by user research + approval (spec Changes v4.1). Facts verified from registry source (`GlobalHotKeyManager::new()/register()/unregister()`, no `HotKeysManager`/`set_shortcuts`); events via `GlobalHotKeyEvent::receiver()` — crossbeam blocking recv, `{id, state Pressed|Released}`; `set_event_handler` exists but steals receiver events, so we use receiver + thread-local debounce (no shared Mutex).
- Key types = `keyboard_types 0.8.3` re-export: `Code` (NOT `Key`): PrintScreen, KeyA..Z, Digit0..9, F1..F12, Unidentified; `HotKey::new(Option<Modifiers>, Code)`, id = `(mods.bits()<<16)|code`, Display "alt+PrintScreen". Modifiers bitflags ALT/CONTROL/SHIFT/META — Win/Command map to META (HYPER/SUPER deprecated aliases folded onto META).
- Backends: windows = hidden window created **on the calling thread** (`global_hotkey_app`, WS_EX_NOACTIVATE), RegisterHotKey(MOD_NOREPEAT|mods, SUPER→MOD_WIN); crate does NOT pump messages — a Win32 message loop must run on that thread (arrives with eframe/winit; thread-affinity decision in Stage 8). linux = X11 (x11rb) with crate-spawned pump thread; register() sends ThreadMessage and waits for reply. QUIRK (x11/mod.rs): if pump thread died (no X server), send error is ignored and register() returns Ok(()) — silent success; our hotkeys.rs probes `$DISPLAY` + `/tmp/.X11-unix/X<n>` before registering (typed `Error::Unavailable`). Pure X11 supported; abstract-socket-only X servers = false-negative risk; Wayland-native unsupported. macos needs manager on main thread + Cocoa run loop (out of scope).
- Error enum `#[non_exhaustive]` thiserror: OsError, HotKeyParseError/UnrecognizedHotKeyCode/EmptyHotKeyToken/UnexpectedHotKeyFormat, FailedToRegister(String) ("Unable to register hotkey: {0}"), FailedToUnRegister, AlreadyRegistered.
- Config schema notation: `key` = Code names case-insensitive ("PrintScreen", "A".."Z", "0".."9", "F1".."F12"); `mods` ∈ {alt, ctrl|control, shift, win|windows|command|meta|super}. Rect combo parsed strictly but typed Reserved (never registered/fires). Debounce 250 ms in bindings module (injected clock; exactly-250ms boundary allowed; blocked repeats do not shift window; per-action independent).

## Research round 6 (2026-11-03, доработки v3)
- **Delivery**: user runs the exe directly from the build — no zip needed (§10: deliverable = `target/x86_64-pc-windows-gnu/release/virshot.exe` + README.md).
- **Item 1 (black console window)**: rustc emits CONSOLE subsystem by default for binaries. We build windows-gnu (mingw), so the correct link arg is `-Wl,--subsystem,windows` (the reddit `/SUBSYSTEM:WINDOWS` variant is msvc-only). Place: `[target.x86_64-pc-windows-gnu] rustflags` in `.cargo/config.toml`. Verify: PE header `subsystem` field of the built exe should be 2 (WINDOWS_GUI). CLI caveat: `virshot shot` stdout only visible when launched from an existing console (cmd/PowerShell); double-click = no console, silent — document in README.
- **Item 2 (PrintScreen rebind dead)**: a registered global hotkey consumes the key press (WM_KEYSHOT → WM_HOTKEY), so pressing PrintScreen during bind-by-press never reaches the egui window (and the registered full-screen action fires instead). Fix direction: during bind-by-press temporarily unregister ALL hotkeys, restore the applied settings after commit/cancel. global-hotkey API: verify `unregister(&HotKey)` / `unregister_all()` in locked 0.9.0 source; manager thread affinity — GUI runs on the main thread where the manager lives, so unregister/re-register from App::logic()/events is legal.
- **Item 3 (toast invisible on Windows)**: current Stage 7 toast = one `show_viewport_deferred` viewport (transparent root flag + passthrough + always-on-top); spec §4.3 already has corners + `on_all_monitors`, but impl is single-toast (Stage 7 limitation). Trigger path: capture thread success → ToastMailbox → gui logic() shows viewport. Possible invisibility causes to diagnose on Windows host: (a) viewport builder needs `with_transparent(true)` on the VIEWPORT builder (not only root), (b) deferred viewport from hidden parent window, (c) Windows transparency support in winit/glow may not produce alpha — solid panel bg may be the pragmatic route (tk reference uses alpha 0.85 on opaque bg), (d) toast must appear only after a successful screenshot (already the trigger — keep).
- **User reference `.tmp/huya-pichot/hua_pichot.py`** (tkinter corner alerts): per-monitor via EnumDisplayMonitors → rect per physical monitor; window per monitor at `right-w-offset, bottom-offset` (offsets 30/60); `overrideredirect(True)` (frameless), `-topmost True`, `-disabled True` (no focus steal), `-alpha 0.85`, solid bg #CC0000 + bold labels, blink 500ms toggle, auto destroy after delay. Mapping: xcap::Monitor::all() → x,y,width,height per monitor (px); per-monitor viewport per corner; eframe equivalents: `with_always_on_top_dd`, `with_mouse_passthrough`, undecorated.

## Research round 7 (2026-11-03, доработки v4)
- **Item 1 (toast "shadow only", через раз)**: user screenshots (.tmp/v4-shot-shadow.png — white empty rounded strip over app; .tmp/v4-shot-taskbar.png — thin strip at taskbar). Symptom matches a created-but-never-painted transparent Windows window (DWM glass/shadow) — paint pipeline stalls for the toast viewport when settings root is hidden (500ms logic tick, 100ms invisible-window throttle, fade uses ctx.input time; if animation/tick stalls, no repaint → unpainted white window). Suspects to verify in source: child-viewport paint gating when root hidden (Stage 19 RequestPaintWhileHidden — drained/re-sent correctly?), fade-in alpha ramp making chip invisible until ramp completes (TOAST_FILL.gamma_multiply(alpha) from 0), window-create flash before first cb paint, ViewportBuilder `with_inner_color` as immediate dark fill fallback.
- **Item 2 (taskbar)**: xcap Monitor rect = FULL monitor incl. taskbar area; toast at bottom corner ends behind taskbar. Windows OS truth = monitor WORK area: `EnumDisplayMonitors` + `GetMonitorInfo` `rcWork` (tk reference script used full Monitor rect — same bug there). No existing API in xcap/winit/egui exposes work area. `windows` and `windows-sys` are ALREADY in Cargo.lock (transitive via winit/tray-icon/rfd) — a direct cfg-gated `windows-sys` dep (Win32 features only) needs explicit user approval per AGENTS.md; no native build deps; non-Windows fallback = full rect (Linux: no taskbar concept; Wayland ignores placement anyway).
- Toast code anchors (gui.rs): TOAST_FILL rgb(24,24,24) :136; chip paint :854; RequestPaintWhileHidden :869; toast_positions :204; monitor_rects :254 (xcap px ÷ scale_factor).

## Research round 8 (2026-11-03, user clarification v4)
- User stopped Stage 21 subagent mid-run; uncommitted partial edits stand in working tree: src/overlay.rs removed ToastPhase::FadeIn (chip legible on first frame — gamma_multiply scaled alpha channel too, early paint rendered nothing), src/gui.rs +67 lines (inner-color/paint changes). Continue FROM these edits, don't rewrite.
- User observation: toast content goes UNDER other windows; impression toast is drawn from TWO windows; directive — remove the shadow window, keep ONLY the toast, always-on-top, offset by taskbar height.
- "Two windows" candidates in current impl (gui.rs Stage 19): (a) replacement closes old toast viewports via ViewportCommand::Close — processed at end of pass, so old+new can co-exist briefly (old unpainted white = "shadow"); (b) fallback chain (xcap fail → egui monitor_size → unpositioned) could spawn a second/unpositioned viewport; (c) hidden ROOT settings window painted via RequestPaintWhileHidden (should stay invisible — verify Visible(false) hide path vs paint side effects). Verify which and ensure exactly ONE toast window per monitor, spawn only after old closed (defer spawn to next pass if needed).
- Always-on-top under games/fullscreen: builder `with_always_on_top()` already set; user says content under windows — verify WS_EX_TOPMOST actually applied for deferred viewports (winit window attributes in egui-winit), and that the toast window is not created as child-owned non-topmost.

## Research round 9 (2026-11-03, Stage 21 verdict + v4 clarification 2)
- Stage 21 verdict (locked sources): white "shadow" = the toast window itself never painted — repaint request sent in spawn pass before any window existed → EventResult::Wait (eframe run.rs:371-386), request dropped AND repaint_delay pinned to ZERO (egui context.rs:166-172) dedups later requests; DWM shows unpainted glass. "Two windows" = replacement only: eframe initializes new viewport windows BEFORE destroying gone ones (glow_integration.rs:1601-1603), unused child dropped at pass end (context.rs:3262-3275).
- egui 0.37 has NO per-viewport inner color (viewport.rs builder — no field/method). Alternative shipped: toast window created with_visible(false)+with_active(false), painted via RequestPaintWhileHidden, shown Visible(true) only after ui callback ran (painted flag). with_always_on_top → WS_EX_TOPMOST chain verified (egui 669 → egui-winit 2301 → winit flags → window_state 276); with_active(false) → SW_SHOWNOACTIVATE (no focus steal).
- v4 clarification 2: windows-sys REJECTED by user (unsafe FFI vs forbid(unsafe_code)). Taskbar handling = empirical corner offsets per tk reference: offset_x=30 (from vertical border), offset_y=60 (from horizontal border) replacing spec's 16px; taskbar ~48px cleared by offset_y=60 at bottom corners. Pure change in toast_positions/constants + tests.

