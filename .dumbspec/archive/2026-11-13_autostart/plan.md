# 2026-11-13_autostart: implementation plan (Добавление в автозагрузку)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan

- [x] draft.md written from raw input
- [x] research.md written (config/GUI/startup flow, cfg-gating template, deps, docs drift, test conventions, gates)
- [x] spec.md v1 (open questions resolved; dependency choice `winreg` flagged for user approval)
- [x] plan.md written

**Criterion:** all four artifacts exist under `.dumbspec/current/2026-11-13_autostart/`; spec v1 has `Created` + `Changes`; plan stages have criteria and (except Stage 0) empty commit lines.
**Commit:** `docs(dumbspec): autostart task — draft, research, spec v1, plan`

## Stage 1 — Config option `general.autostart` (Windows-only field)

- [x] Red: sibling tests in `src/config.tests.rs` — Windows-gated roundtrip (`autostart` serialize/parse), non-Windows test asserting a TOML without the key parses to defaults and the field is absent from `General` on Linux builds; defaults test updated to cover the option.
- [x] Green: `#[cfg(windows)] autostart: bool` field in `General` (`src/config.rs`), struct-level `#[serde(default)]` keeps partial TOML working; `Default`/`general_with_home` set `false`; wire module test attachment unchanged.
- [x] Refactor + iteration gate `./scripts/check.sh` green (Linux compile + tests + clippy).

**Criterion:** `./scripts/check.sh` passes on Linux; `./cargo.sh check --target x86_64-pc-windows-gnu` compiles; Windows-gated roundtrip test compiles under `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`; Linux TOML schema (no `autostart` key) untouched on Linux saves.
**Commit:** `feat(config): add Windows-only autostart option`

## Stage 2 — Module `src/autostart.rs` (+ dependency `winreg`, pending user approval)

- [x] Red: sibling `src/autostart.tests.rs` (Linux-runnable via `#[cfg(any(test, windows))]` pure helpers) — value-name constant, payload computation from injected exe path (quoting), decision table write/delete/skip from current-vs-desired value.
- [x] Add dependency: `[target."cfg(windows)".dependencies] winreg = "0.56"` (0.56.0, feature-empty — no default features to trim; user-approved) in root `Cargo.toml`; `Cargo.lock` update.
- [x] Green: `src/autostart.rs` — typed `thiserror` error enum; `#[cfg(windows)]` registry read/write/delete on `HKCU\...\Run` value `virshot`; `#[cfg(not(windows))]` no-op stubs (dual pattern of `src/gamepad.rs`); declared from `src/main.rs`. Items gated `#[cfg(any(test, windows))]` (Linux production compiles none); Windows integration test uses `TEST_VALUE_NAME` under `cfg(test)` so test runs never touch the user's real entry.
- [x] Windows-gated integration test (`#[cfg(windows)]`) against a dedicated scratch Run value name (`TEST_VALUE_NAME` under `cfg(test)`), cleanup on drop — implemented and compiles under windows-gnu; execution happens on Windows hosts/CI.
- [x] Refactor; gates: `./scripts/check.sh` + `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green (Linux: 610 passed, clippy clean; windows-gnu test binaries compile).

**Criterion:** Linux check/test/clippy green; pure decision tests pass on Linux; windows-gnu compile (incl. `--no-run` tests) green; module has no `unsafe` (winreg internals are third-party).
**Commit:** `feat(autostart): registry sync module over winreg` — landed as `ea47b77`

## Stage 3 — Startup sync in `run_resident`

- [x] Red/verify: behavior expectation — sync called after `config::load_or_create` + instance guard only on Windows; failure logs ERROR and startup continues (no panic, no exit). (Thin cfg-gated wiring; pure logic already pinned by `src/autostart.tests.rs`; no extra helper — YAGNI.)
- [x] Green: wired `autostart::sync(settings.general.autostart)` in `run_resident` (`src/main.rs:462-475`) right after the instance-guard log — exactly one resident process mutates the registry; INFO on success / typed ERROR + continue on failure; `run_shot` untouched. `TEST_VALUE_NAME` gate narrowed to `#[cfg(test)]` → Windows build warning-free.
- [x] Gate: `./scripts/check.sh` green (610 passed, clippy clean); `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green, 0 dead-code warnings.

**Criterion:** on Windows startup the Run value matches `general.autostart` (present iff true, absent iff false), including after manual config edits; broken-config defaults path yields removal of a stray entry; Linux startup unchanged.
**Commit:** `feat(main): sync autostart from config on startup` — landed as `248c444`

## Stage 4 — GUI checkbox + save-time sync

- [x] Red: GUI-side expectation tests — red first (`test --no-run` failed 4× E0425 for missing label helpers), then `src/gui/general.tests.rs` `autostart_label_and_placeholder_render_in_every_locale` + `src/config.tests.rs` `autostart_accessors_on_linux_read_false_and_ignore_writes`; dirty detection covers the new bool via plain draft-vs-applied PartialEq (no special-case).
- [x] i18n: keys `general.autostart_label` and Windows-only-disabled label in `locales/app.yml` for en/ru/zh-CN/es/ja/ko (+ `src/i18n.tests.rs` key-count pins 95→97).
- [x] Green: checkbox in `SettingsApp::general_tab` (`src/gui/general.rs:388-421`) after the Update group; on non-Windows disabled `egui::Checkbox` placeholder via `ui.add_enabled(false, …)` (hotkeys.rs pad mechanism); cfg-free GUI via dual `autostart()/set_autostart()` accessors in `src/config.rs` (gamepad.rs-style cfg-gated impls); locale-render test added in `src/gui/general.tests.rs`; Windows clippy drift fixed (`empty_line_after_doc_comments` in config.tests.rs).
- [x] Green: `SettingsApp::apply()` calls `autostart::sync(self.draft.general.autostart())` in the `Ok` arm after `config::save` (`src/gui/app.rs:389-407`), Windows-only; save stays successful if registry sync fails (typed ERROR log, Apply/OK keep working); Cancel never calls apply().
- [x] Gate: `./scripts/check.sh` green (612 passed, clippy clean); windows-gnu `test --no-run` + `clippy --target … --all-targets` green, warning-free.

**Criterion:** checkbox reflects `draft.general.autostart`; Apply/OK toggles the Run value accordingly; Cancel changes nothing; on Linux checkbox is disabled and nothing touches the registry.
**Commit:** `feat(gui): autostart checkbox in General tab with save-time sync` — landed as `50cbcfc`

## Stage 5 — Documentation amendments

- [x] `docs/spec.md`: version-log line naming this task (v11); removed "Автозапуск при логине" from out-of-MVP (§2); added autostart requirement to §4.4 General list; added `autostart` key to §7 TOML schema (Windows-only note); consistency follow-ups: §3 tech-stack `winreg` bullet, §2/§10 locale-count 76/95→97. Russian prose.
- [x] `README.md:27`: amended "без реестра" wording — registry is used only for opt-in Windows autostart.
- [x] Gate: markdown diffs reviewed; no code changes; grep for "реестр/автозапуск/autostart" leftovers clean (only the pre-existing unrelated webbrowser note).

**Criterion:** docs describe the shipped behavior consistently; no leftover "out of MVP"/"без реестра" contradiction.
**Commit:** `docs: spec and README — Windows autostart shipped` — landed as `82e2f73`

## Stage 6 — Final verification

- [x] `./cargo.sh fmt --check` + `./scripts/check_full.sh` (Linux full gate incl. fmt) green.
- [x] `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green (Windows test binaries incl. `#[cfg(windows)]` autostart tests).
- [x] `./cargo.sh clippy --workspace --all-targets -- -D warnings` green.
- [ ] Windows-host acceptance run by user (checkbox → Run value appears/disappears; manual config edit + restart syncs).

**Criterion:** all gates green; feature verified on Windows hardware by the user (registry entry created/removed per checkbox and config).
**Commit:** N/A
