# Research — settings-first-start-only (English, facts only)

Source: research subagent run 2026-11-15 + orchestrator caller grep. Repo: /home/apkawa/code/vibe-shot.

## Config module — `src/config.rs`

- `load(path: &Path) -> Result<Settings, Error>` (config.rs:93): TOML read; missing file → `Error::Io { not_found: true }` (`is_missing` :61); absent keys → spec §7 defaults via `#[serde(default)]`.
- `load_or_default(path) -> Settings` (:140): never fails; missing file → `Settings::default()`, no log (`fallback_reason` :104 returns `None` for `not_found`); parse/io failures log ERROR; normalizes `general.language` (`language_code` :121).
- `load_or_create(path) -> Settings` (:165): `load_or_default` then `if !path.exists() { create_default_file(path) }` (:167–169). Existence checked **after** load; **no "was created" signal returned**. Corrupt/unreadable file is deliberately NOT rewritten (:159–164).
- `create_default_file(path)` (:176): `create_dir_all(parent)` + `save(&Settings::default(), path)`; failures logged WARN, never returned.
- `save(settings, path) -> Result<(), Error>` (:193): pretty TOML; called by GUI Apply (gui/app.rs:425).
- `default_config_path() -> PathBuf` (:370): Windows portable `<exe dir>/config.toml`; fallback `%APPDATA%\virshot\config.toml`, then `./virshot/config.toml`; Linux `$XDG_CONFIG_HOME`/`$HOME/.config`/`.config` + `virshot/config.toml`. CLI `--config` overrides (main.rs:192).
- Defaults: `General::default` (:499), `Controls::default` (:596), `Overlay::default` (:624), `Settings` (:638).

## Startup flow

- `main()` (main.rs:248): CLI → `--version` exit → logging init → startup update cleanup → dispatch `run_shot` (:323, headless) or `run_resident(config_path)` (:303).
- `run_resident` (main.rs:428): `config::load_or_create` (:429) → locale (:436) → `InstanceGuard::acquire` (:448, lock `<config_path>.lock`) → autostart registry sync (Windows, :469) → hotkeys (:490) → gamepad pump (:512) → tray (:521) → update channel (:539) → **unconditional `gui::run(...)` (:544)**. GUI failure → `park_without_gui` (:566/:668).
- `gui::run` (gui/window.rs:140, re-exported from gui/mod.rs:105): `egui::ViewportBuilder` (:153–176) — title, 720×520, `with_transparent(true)`, no minimize/maximize buttons. **No `with_visible(false)`** → root viewport opens visible. Create closure installs fonts, `OverlayHost` + `ToastService` (:200–208), `capture::attach_handoff` (:208), `SettingsApp::new` (:218).
- **Today the settings window opens on every resident startup** — no `first_run` flag, no condition around `gui::run`. CLI surface: `--version/--config/--log/--log_level` + `shot` subcommand (main.rs:179–228).

## Callers (orchestrator grep, 2026-11-15)

- `config::load_or_create` callers: main.rs:329 (`run_shot`) and main.rs:429 (`run_resident`); tests config.tests.rs:592/:607/:623/:638.
- `gui::run` call site: exactly one, main.rs:544 (`run_resident`).

## Existing "first run" notion

- Only existence check tied to config: `load_or_create`'s `!path.exists()` (config.rs:167). No `first_run` variable/flag anywhere.

## Tray / reopen path

- `src/tray.rs`: `TrayAction` (:36 — `ShowSettings`, `TakeScreenshot`, `About`, `Exit`); left-click UP → `ShowSettings` (:150); menu `virshot-settings` → `ShowSettings` (:161); `TrayController::new` (:187, main thread, before GUI); `poll` (:227).
- `SettingsApp::logic` (gui/app.rs:517–566): `ShowSettings` → `ViewportCommand::Visible(true)` (:522) + repaint; `Exit` → `exit_requested` + `ViewportCommand::Close` (:559–562); X-close → hide-to-tray (`chrome::close_decision`, `CancelClose` + `Visible(false)`, :963–964); hidden loop stays alive via `TRAY_POLL_TICK` 500 ms (:104, :1019).
- GUI-less park loop (main.rs:778–803): `ShowSettings`/`About` only WARN (:796–801) → skipping `gui::run` entirely would break tray reopen.

## Tests

- `src/config.tests.rs`: `load_or_create_missing_file_creates_file_with_defaults` (:589), `load_or_create_corrupt_file_returns_defaults_without_rewrite` (:601), valid-file no-rewrite (:618), parent-dir creation (:634).
- `tests/e2e/cli_headless.rs` (:144–157): headless `shot` asserts missing config auto-created with defaults.
- No test asserts startup window visibility today.

## docs/spec.md (Russian) — stated intended behavior

- §2 line 125: tray MVP — «Работа в системном трее (**скрытый запуск**, контекстное меню)» — hidden startup is an MVP feature; current code does not honor it.
- §2 :126 + §7 :922–925: config auto-creation of defaults on first run; broken config → ERROR log + run on defaults.
- §4.4 (:448): chrome + X-close hides to tray; no statement about startup visibility. §4.5 (:627): tray left-click opens settings.
- §5 (:887)/§6 (:891): resident without GUI when GUI fails.
- Autostart (HKCU `Run`, main.rs:469) launches the resident app at login — today also opens the settings window.

## Fixed constraints

- Thread model (AGENTS.md): exactly one winit `EventLoop` per process; `OverlayHost` + `ToastService` + toast-handoff attach live inside `gui::run`'s create closure (window.rs:185–247) ⇒ skipping `gui::run` skips toast wiring and tray reopen; keeping `gui::run` with a hidden viewport preserves both.
- `#![forbid(unsafe_code)]`; unit tests TDD, pure logic, deterministic; gates `./scripts/check.sh` (iteration) and `./scripts/check_full.sh` (commit/stage boundary); `./cargo.sh` wrapper for sandbox caches.
- No new dependencies needed for this change.

## Post-execution finding (2026-11-15, user reports window still opens) — eframe first-paint reveal

- `.with_visible(false)` reaches winit faithfully: egui-winit 0.37 lib.rs:2292 `.with_visible(visible.unwrap_or(true))` → winit Windows window.rs:1263 `win.set_visible(attributes.visible)` → window_state.rs:392-395 `ShowWindow(SW_HIDE)`. Root window is created hidden.
- BUT eframe 0.37 glow reveals the root after its FIRST painted frame: epi_integration.rs:294 `is_first_frame: true` consumed once in `post_rendering` (:442-449) → `window.set_visible(true)` ("We keep hidden until we've painted something", PR #2279).
- On Windows the first paint is synchronous at `Event::Resumed` (run.rs:123-134 Full pass), and a hidden window is treated paintable: `viewport.info.visible()` (egui viewport_info.rs:98-104) derives visibility from minimized/occluded only; winit Windows never emits `Occluded` (egui-winit `update_viewport_info` never sets it) → `None` → `unwrap_or(true)` → paint happens.
- Consequence: `visible=false` cannot survive startup; `is_first_frame` is taken once, so later `ViewportCommand::Visible(false)` (close-to-tray) still works. eframe persistence feature is OFF (Cargo.toml:26 features glow/default_fonts/x11/wayland/links) — no restored-window interference.
- Suppression strategy (Stage 4): app in startup-hidden state sends `ViewportCommand::Visible(false)` every logic pass until first tray `ShowSettings` clears the state; logic runs before paint per frame, so only the first pass can lose the race (reveal in post_rendering of the Resumed pass) — hide lands within the next pass (ms), no user-visible window on screen.
- Secondary pitfall (not a bug): Windows portable config lives NEXT TO THE EXE (config.rs candidates :244-254, probe_dir_writable :272-284); running a fresh exe copy from another dir creates a new config → `first_run=true` by design. Instance guard is per config path (main.rs:454).
- Update auto-install restart (main.rs:547 arm_startup_check) re-enters startup in the new process → reveal via the same mechanism rides on the fix.

## Stage 5 research (2026-11-15): tray-first settings window — existing child-viewport pattern + constraints

- About pattern (src/about.rs + app.rs:639-684): open/close state in app flags (`AboutWindow`, fresh `close_flag` per open), close observed INSIDE child paint via `ui.input(|i| i.viewport().close_requested())` (about.rs:235-237), un-register next pass ⇒ egui GC ⇒ OS window destroyed (context.rs:3246-3270, glow_integration.rs:1533-1552); already-open ⇒ `ViewportCommand::Focus` (app.rs:588-591); builder constant per viewport id (chrome flags trigger window recreate, egui viewport.rs:865-952); child fills opaque CentralPanel (about.rs:240-241) because children inherit ROOT `transparent` (glow_integration.rs:1630-1643, window.rs:166).
- Chip recipe for a HIDDEN child (crates/overlay/src/host.rs:370-387, 496-517): spawn `show_viewport_deferred` with `.with_visible(false)`; while hidden, send `RequestPaintWhileHidden` to ROOT (host.rs:538-541, gated on ROOT `visible()==Some(false)`) to get a full pass — logic-only pass NEVER creates windows (glow :707-757, LogicOutput has no viewport-existence info, output.rs:84-93); child paint gated by child's own RequestPaintWhileHidden (:505), then `Visible(true)` after first paint (:507); throttle `request_repaint_after_for(INTERVAL, viewport)` re-issued per pass, guarded by `raw.viewports.contains_key(id)` (:406-410; commands pin delay to ZERO, egui context.rs:133-172).
- Reveal flag `is_first_frame` is ONE per app (epi_integration.rs:183/:294), consumed by whichever window paints (glow :931); at startup the hidden ROOT's Resumed full pass consumes it (suppression re-hides within ms), so afterwards children painted while hidden are NOT auto-revealed — explicit `Visible(true)` shows them cleanly, no flash.
- `CancelClose` is ROOT-only (epi_integration.rs:428-435); child X delivers `ViewportEvent::Close` to the child's info (glow :1075) and a HIDDEN child's close event is dropped (logic-only branch clears events, :724). Child hide = paint records close → `send_viewport_cmd_to(child, Visible(false))`.
- Root `App::ui` still runs on full passes even when a child is visible (is_viewport_or_descendant_visible, glow :1689-1704) → root ui must become empty when settings UI moves out.
- Persistence: eframe autosaves the PAINTING window's geometry into one STORAGE_WINDOW_KEY (glow :966, epi_integration.rs:470-490) — child paints write root's slot; restore applies to whichever window consumes is_first_frame (root at startup). Out of scope; harmless (root hidden).
- Tray ShowSettings semantics: hidden alive child → `Visible(true)`; visible → `Focus`; destroyed→respawn (About precedent). Exit stays root-bound (tray Exit → root Close; OK/Cancel request_window_exit → root Close + exit flag).
- Bind-by-press capture reads per-viewport events (hotkeys.rs:284-340) → works only with child focused; rfd Browse blocks the child's pass only.
- Parked mode (GUI err) unchanged: no settings UI there.
