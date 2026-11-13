# Research — autostart (Windows) — context collection

Task: `2026-11-13_autostart`. Superficial scan only (deps, files, fixed constraints, existing solutions). All paths/lines verified by the research agent.

## Config module — `src/config.rs` (608 lines, bin-only crate, items `pub(crate)`)

- `Settings` (:598-604) = `{ general: General, controls: Controls, overlay: Overlay }`, struct-level `#[serde(default)]` → partial TOML falls back per section.
- `General` (:473-487): `language`, `save_path`, `file_full_screen`, `file_window`, `format: Format`, `jpeg_quality`, `webp_quality`, `update_check: UpdateCheck`, `update_channel: UpdateChannel`, `auto_install: bool`. Manual `Default for General` (:489-495) delegates to pure `general_with_home(home)` (:501-512) — precedent for env-resolved defaults.
- serde/toml: derives at :16; `toml::from_str` in `load` (:93-97); `toml::to_string` in `save` (:193).
- Defaults/validation: absent keys → defaults; lenient repair only for `general.language` (`language_code` :121-129 applied in `load_or_default` :140-152); strict enums fail whole parse → typed ERROR log + `Settings::default()`. No clamping — "save validates nothing" convention; GUI-side gates instead (`binding::draft_savable`, `duplicate_bindings`, `src/gui/mod.rs:8`).
- `load_or_create` (:165) auto-creates defaults on first run, never rewrites corrupt files; `save` (:193) creates parent dirs, typed `Error::Io`/`Error::Parse` (enum :26-48). `config::save` is called from exactly one place: `SettingsApp::apply` (`src/gui/app.rs:373`) + internal first-run creation.
- Path resolution: Windows portable mode = `config.toml` beside exe, fallback `%APPDATA%\virshot\` (:200-330); pure env-driven `config_path_from_env` (:219).

## GUI — `src/gui/` (module directory: app/binding/chrome/general/hotkeys/icons/overlay/update/window)

- Tabs: `enum Tab { General, Hotkeys, Overlay }` (`app.rs:106-121`), dispatch at :1054-1069; default `Tab::General` (:318). Settings window built in `src/gui/window.rs:140`.
- General tab paint: `SettingsApp::general_tab` (`src/gui/general.rs:150-~380`); last group is the Update group with `ui.checkbox` (:323, `auto_install`) + conditional rows (:336+).
- Checkbox convention: plain `ui.checkbox(&mut bool_field, rust_i18n::t!("..."))` — precedents `general.rs:323`, `overlay.rs:94`, `overlay.rs:141`. No abstraction.
- Draft/save flow: `SettingsApp` owns `config_path`, `applied: Settings`, `draft: Settings` (`app.rs:127-132`); dirty = `chrome::is_dirty(draft, applied)` (`gui/chrome.rs:7`); bottom bar Apply/OK/Cancel gated by `chrome::bottom_buttons_enabled` (`chrome.rs:24-32`). Persisting path = `SettingsApp::apply()` (`app.rs:362-422`): duplicate-bindings guard → `config::save(&self.draft, &self.config_path)` (:373) → locale switch (:381) + tray rebuild (:385) → `applied = draft.clone()` (:392) → update-schedule rearm (:402) → hotkey re-registration (:405-409). OK calls `apply()` then closes (:1037-1041); Cancel never saves (:1043-1050).
- i18n: all labels via `rust_i18n::t!`, dotted keys in single catalog `locales/app.yml` (`_version: 2`, locales en/ru/zh-CN/es/ja/ko; e.g. `general.auto_install_label` at :188). New checkbox needs keys in all 6 locales.
- Windows-only UI precedent: runtime `cfg!(windows)` inside the paint (pad field renders disabled "(gamepad — Windows only)" placeholder on Linux, `src/gui/hotkeys.rs:544-560`). No compile-time `#[cfg(windows)]` in `src/gui/`; GUI modules always compile on Linux.

## Startup flow — `src/main.rs` (825 lines, `#![forbid(unsafe_code)]` at :63)

- `main()` (:241): CLI parse; `--version` short-circuit; logging init before config load (:259-264); startup update cleanup (:272-285, uses `std::env::current_exe()` :278 — exe-path precedent); dispatch `run_shot` (:315) or `run_resident` (:317).
- `run_resident(config_path)` (:421): 1) `config::load_or_create` (:422); 2) locale sync from config (:429) — sync-from-config precedent; 3) single-instance guard (:445-465); 4) settings mailbox `Arc<Mutex<Settings>>` (:475); 5) `hotkeys::HotkeysController::start` (:480); 6) `gamepad::spawn_pump` (:491, cfg-gated, Linux no-op + INFO log); 7) tray (:500-510); 8) `arm_startup_check` (:518, pure decision fn `should_check_at_startup` :155 — config-driven startup behavior precedent); 9) `gui::run` (:523); `park_without_gui` (:679) on GUI failure.
- Headless `run_shot` (:322) loads config but deliberately does no state syncing beyond CLI overrides.
- Natural startup hook point: after config load / instance guard in `run_resident`, same shape as locale sync (:429) and check arming (:518).

## Windows-specific code today

- Target-scoped path deps: `[target."cfg(windows)".dependencies] gamepad = { path = "crates/gamepad" }`; `crates/video-capture` same shape for `windows-capture = "2.0.1"`. Linux gate never compiles them.
- `src/gamepad.rs` (726 lines) is the cfg-gating template: `#[cfg(windows)]` imports (:80-100); dual `spawn_pump` — real (`#[cfg(windows)]` :437+) vs log-and-nothing (`#[cfg(not(windows))]` :474+); pure helpers exposed to Linux tests via `#[cfg(any(test, windows))]` (:238-333).
- `src/instance.rs:168` `#[cfg(windows)] fn probe_pid_alive`; `src/hotkeys.rs` gates behavior with runtime `cfg!(windows)` (:572).
- **Registry access: none today.** No `winreg`, no HKCU usage anywhere. README.md:27 promises portable "**без реестра**" and docs/spec.md:134 lists "Автозапуск при логине" as out-of-MVP — both docs need amendment when this ships.

## Dependencies

- Registry-related deps in Cargo.lock: no `winreg`. `windows` 0.62.2 (+ windows-core, windows-result/strings/collections/targets) is transitive only via xcap and windows-capture; `windows-sys` various versions transitive via winit/rfd/tray-icon/global-hotkey/eframe/etc. None enable registry APIs for project code.
- → Any registry access needs a NEW dependency declaration (`winreg`, or `windows`/`windows-sys` with Win32_System_Registry features) → per AGENTS.md, new deps require explicit user confirmation.
- Zero-dep alternative precedent: hand-written gameinput.dll shim in `crates/gamepad` — but that needs `unsafe`, forbidden in root crate.
- Budget: exe < 12 MB (spec §5); Cargo.toml comments track dep sizes (serde-saphyr +22 KB on windows-gnu). Feature-trimmed deps convention.

## docs/spec.md (1161 lines, Russian)

- §1 Обзор (:72); §2 Цели и ограничения (:80, out-of-MVP list :122-149 — autostart bullet at :134); §3 Стек (:151); §4 Функциональные требования (:236; §4.4 Окно настроек :425, General tab enumerated :463-477); §5 Нефункциональные (:819); §6 Архитектура (:845); §7 Формат конфигурации (:878, TOML schema block :898-933, keys lowercase English); §8 CLI (:935); §9 Ограничения (:979); §10 Тестирование/поставка (:1073).
- Header carries dated version-log lines (v4..v10), each naming a task id — that is how features are announced in spec.md.
- No autostart requirement anywhere except the out-of-MVP bullet (:134).

## Test conventions in practice

- Root package is bin-only (no `lib`); `tests/unit/main.rs` is a placeholder. All unit tests are sibling `#[cfg(test)] #[path = "<module>.tests.rs"] mod tests;` attached at module end (`src/config.rs:606-608` etc.).
- Naming `<unit>_<scenario>_<expectation>`; pure logic only; injected env/time; scratch fixtures in `./.tmp/` (ScratchDir with Drop cleanup, `src/gui/general.tests.rs:23-41`); `/`-normalised path asserts (`normalised()` :18-21); locale-touching tests serialize on `crate::LOCALE_LOCK`.
- Relevant config tests (`src/config.tests.rs`, 924 lines): defaults-vs-spec §7 (:5), partial→defaults (:461), corrupt→defaults (:549), load_or_create semantics (:583-643), save/load roundtrip (:645), strict update enums (:823-880).
- Platform-gated patterns: `#[cfg(not(windows))]` test (`config.tests.rs:129`), runtime `cfg!(windows)` branch in tests (:112-127), `#[cfg(windows)]` test (`bindings.tests.rs:646`), Linux-side gamepad tests rely on `#[cfg(any(test, windows))]` helpers.
- Gates: Linux `./scripts/check.sh` = check + root tests (SKIP_E2E filters e2e) + clippy `-D warnings`; crates separately; Windows gate compile-only: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`; CI Windows job executes tests on Windows runners.

## Constraints & gotchas

- `#![forbid(unsafe_code)]` at `src/main.rs:63` — registry FFI cannot live in `src/` (nested allow cannot relax forbid). Documented exception shape: separate crate (`crates/gamepad/src/lib.rs:19` deny + module-scoped allow in gameinput.rs; overlay same). So registry code = new approved crates.io dep (pure-Rust wrapper keeps root crate clean of `unsafe` — third-party unsafe is explicitly outside the rule) OR a new path-dep crate mirroring gamepad.
- Linux gate must compile everything: registry module needs `#[cfg(windows)]` internals + non-Windows no-op stub (gamepad.rs dual-function pattern); pure logic testable on Linux via `#[cfg(any(test, windows))]`.
- Windows cross-build target `x86_64-pc-windows-gnu` (MinGW linker); release.yml builds windows-msvc + linux-gnu; CI Windows job runs real tests. New dep must compile on windows-gnu (pure-Rust crates do).
- One module = one file (AGENTS.md); `src/gui/` is the established directory exception. New root module = `src/<name>.rs` declared from `main.rs`.
- Config validation convention: bool option follows `auto_install` precedent (plain bool, struct-level serde default, localized label; save validates nothing).
- Save hook = exactly `SettingsApp::apply()`; startup hook = `run_resident` after `config::load_or_create` + instance guard.
- Broken config → defaults + ERROR log, never rewritten (config.rs convention) — startup registry sync must tolerate defaults fallback.
- Registry value needs `std::env::current_exe()` (precedent `main.rs:278`).
