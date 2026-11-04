# 2026-11-03_gamepad-integration: implementation plan (gamepad library + virshot integration)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan

- [x] Capture raw input into draft.md (incl. user supplement: examples console app)
- [x] Branch `2026-11-03_gamepad-integration` created off master (clean tree)
- [x] Superficial research pass via subagent; findings appended to research.md
- [x] Spec review decisions recorded (shim vs windows crate, pad bind format, Linux out of scope)
- [x] spec.md v1 (Created + Changes) written
- [x] plan.md written

**Criterion:** draft.md, research.md, spec.md (v1), plan.md exist in `.dumbspec/current/2026-11-03_gamepad-integration/`; no blocking open questions.
**Commit:** `5585edd docs(dumbspec): gamepad-integration task — spec v1 + plan` (done)

## Stage 1 — Pad bind semantics (pure logic, TDD)

- [x] Red: tests for pad notation parsing — canonical names (`South`, `North`, `East`, `West`, ...), `unknown:<N>`, `Guide`, `Share`, invalid values rejected (config-level, Linux-runnable)
- [x] Green: extend config `Binding.pad` model into a parsed pad-bind representation (canonical name → canonical button id; `unknown:<N>` → raw index; `Guide`/`Share` → system button)
- [x] Refactor: keep `pad` serialized in TOML as the string notation; bindings module consumes parsed pad alongside keyboard OR-fire, debounce unchanged (250 ms)
- [x] Gate: `./scripts/check.sh` green (linux); `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` compiles

**Criterion:** `./scripts/check.sh` passes; unit tests cover canonical-name mapping, `unknown:<N>`, Guide/Share, invalid input; existing keyboard-hotkey behavior untouched.
**Commit:** `feat(config): gamepad pad bind notation parsed from config`

## Stage 2 — `crates/gamepad` library (copy poc nearly 1:1)

- [x] Create `crates/gamepad/`: Cargo.toml (zero deps, edition 2024, own manifest, not workspace member), copy `vendor/GameInput.h` + NOTICE.md, copy `src/layout.rs` (+ its 8 offset tests), copy `src/gameinput.rs` from poc
- [x] Facade `src/lib.rs`: crate root `#![deny(unsafe_code)]`; single `#![allow(unsafe_code)]` exception kept on `gameinput.rs`; public API = observer trait (`device_connected`, `buttons_changed(bits, indices)`, `system_button(Guide|Share, down)`) + `run(observer)` driving the poc pump loop (focus policy, registrations, dispatcher, 50 ms poll — unchanged mechanics)
- [x] Minimal changes only: poc stdout prints replaced by observer calls; poc build.rs stamp dropped; no new slots added yet
- [x] Labels: header-truth correction — `IGameInputDevice` has NO GetButtonLabel slot (WinRT name); labels read from device info block (v3 controllerInfo→controllerButtonLabels, v2 controllerButtonInfo, v0 None), exposed as `button_label(device_index, button_index)` + pure `button_name(GameInputLabel code)`. Accepted as the Stage-5 label source.
- [x] Tests: layout offset tests pass on linux; pure helper tests for label codes in the crate
- [x] Gates: `cd crates/gamepad && ../../cargo.sh test` green; windows-gnu build + test --no-run compile; root `./cargo.sh check` green

**Criterion:** `crates/gamepad` builds and its tests pass on linux; windows-gnu debug build compiles; lib API exposes observer events and labels; gameinput.rs remains the only unsafe exception. — VERIFIED (acceptance run: 13 tests green, byte-identical layout/vendor, 47 boundary-only hunks, slots/timings untouched).
**Commit:** `ebee936 feat(gamepad): gamepad library extracted from hidpad-poc with observer API` (done; spec-change note in `e46c886`)

Accepted defects to fix in Stage 4 wiring: (a) observer lifetime erasure — callbacks outlive `run`; switch plumbing to `Arc<dyn Observer>` and clear on exit, keep observer alive for process lifetime in virshot; (b) emit holds OBSERVER mutex during callback — callbacks must not re-emit; (c) device index computed under a separate lock from push — make dedup-check+push one critical section; (d) stop-flag granularity ~5 s in fallback mode.

## Stage 3 — Console tester example in `crates/gamepad/examples/`

- [x] `examples/buttons.rs`: console app on the lib API reproducing the poc log format (build-less stamp optional; device lines, `button bits=0x... idx=[...]`, `[ts] name guide/share DOWN/UP`, `alive:` heartbeat)
- [x] Compile: `cd crates/gamepad && ./cargo.sh build --target x86_64-pc-windows-gnu --example buttons`
- [x] Linux: `cargo check --example buttons` compiles (gameinput module cfg(windows) → stub path)
- [x] Extra: label hint per pressed index (`button N = Label` via `button_label`); deliberate deviations documented in-file (SystemTime stamp instead of GameInput u64; v0 field label)

**Criterion:** example binary produced for windows-gnu; source mirrors poc output via the lib observer API; no changes to library internals. — VERIFIED (acceptance run: buttons.exe PE32+ console x86-64 produced; poc formats line-by-line matched; lib internals untouched; 13 tests + clippy green).
**Commit:** `feat(gamepad): console button-tester example`

## Stage 4 — virshot gamepad trigger integration (Windows)

- [x] Root Cargo.toml: path dep `gamepad = { path = "crates/gamepad" }` under `[target."cfg(windows)".dependencies]` (no crates.io deps — approval rule satisfied, no new external crate)
- [x] `src/gamepad.rs` module (`#[cfg(windows)]` wiring): spawn pump thread, observer forwards pad events into the listener side (owned values/channel), pad bind matched via Stage-1 parsed pad + `bindings::Debounce` (250 ms) → `capture::spawn_capture` (same command path as hotkeys)
- [x] Unit tests (Linux-runnable, no hardware): pad-event → action mapping incl. Guide/Share system buttons, OR-fire with keyboard field, debounce behavior
- [x] Device connect/disconnect logged via tracing (INFO), app keeps running on gameinput init failure (errors are values) — connect INFO done; **disconnect INFO deferred to Stage 5** (crate emits only device_connected; needs a crate-level device_disconnected event)
- [x] Gates: `./scripts/check.sh` green; `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` compiles

**Criterion:** linux gate green; windows-gnu compile green; shot fires from a matched pad bind (hardware verification deferred to Windows host run of the example + virshot). — VERIFIED (acceptance: 255 unit + 9 CLI green, windows-gnu compiles gamepad+virshot, one shared 250 ms gate across hotkey/pad sources).
**Commit:** `007aeb7 feat(virshot): gamepad trigger via crates/gamepad pump thread` (done)

Accepted defects to carry into Stage 5: (a) PadObserver holds startup Settings snapshot — pad-triggered shots ignore UI Apply; Stage 5 must use a settings/triggers mailbox like hotkeys.rs (otherwise UI re-bind won't fire); (b) `pressed_indices` = currently-pressed set — fire only on 0→1 transition in `bits` (fix in gamepad.rs, verify on hardware run); (c) crate needs a `device_disconnected` observer event for INFO logging; (d) stop never set = process-lifetime parity (intentional).

## Stage 5 — UI: "press a button" bind capture + labels

- [x] `src/gui.rs`: pad field click enters "press a button" capture mode (Windows): first DOWN edge writes `unknown:<N>` / `Guide` / `Share` into the pad field (replaces placeholder `(gamepad — future integration)`)
- [x] Label hint: show label for captured raw index when available (header-truth: `button_label` from the device info block, not a GetButtonLabel slot; Guide/Share → no hint)
- [x] Tests for capture-state logic (Linux-runnable pure state transitions)
- [x] Gates: `./scripts/check.sh`; windows-gnu compile
- [x] Stage-4 carry-overs: settings mailbox shared hotkeys/pump/GUI (Apply + re-bind fire without restart); 0→1 edge tracker (`newly_pressed = bits & !previous`); `device_disconnected` crate event + INFO; shot suppressed while any bind field captures

**Criterion:** placeholder removed; captured button persists into config `pad` and fires Stage-4 mapping; label hint displayed when label resolves. — VERIFIED (compile+logic; hardware deferred). Escape does not cancel pad capture (reclick/keyboard-capture cancel) — doc line fixed in Stage 6. Post-commit one-frame race, pump-init-failure UX, lowest-index tie-break, Guide/Share hints — notes for the Windows hardware run.
**Commit:** `feat(gui): gamepad button capture for pad bind field`

## Stage 6 — Docs + final gate

- [x] `docs/spec.md`: gamepad integration moves from non-MVP list into implemented section (Windows gameinput shim; Linux gilrs still out of scope w/ hardware note) — v5 changelog block, §1/§2/§3/§4.1/§4.4/§6/§7/§8/§9 updated
- [x] `AGENTS.md`: gamepad dependency notes updated (shim, crates/gamepad path dep; drop stale "via winit" claim — actual: `windows` transitive via xcap 0.9.8, winit uses windows-sys) + stale global-hotkey/tray lines fixed
- [x] README.md stale gamepad lines updated (deviation: criterion is "docs match shipped behavior")
- [x] `src/gui.rs` doc-text fixes only (Escape-cancel claim corrected; duplicated gui::run fragment collapsed; stale GetButtonLabel comments reworded; `src/gamepad.rs` comment nits reworded too)
- [x] `./scripts/check_full.sh` green (fmt at boundary); crate-level fmt/test/clippy green; windows-gnu `test --no-run` green

**Criterion:** docs match shipped behavior; `./scripts/check_full.sh` exits 0; windows-gnu `test --no-run` compiles. — VERIFIED (279 unit + 9 CLI + crate 13, fmt clean, both targets compile). Remaining nit not blocking: crates/gamepad/src/lib.rs:300-304 comment lists IGameInputReading slots on the device block — internal comment in the library, left as-is (Stage 6 was docs-only outside gui.rs/gamepad.rs).
**Commit:** `docs: gamepad integration status in spec and AGENTS`

---

Deferred (not a stage, hardware-dependent): Windows hardware acceptance run
(example `buttons` + virshot pad trigger on user's BT LE XINPUT pad; Share/Guide
behavior per docs/spec.md §9). — SETTLED 2026-11-04: user ran `buttons.exe` on
the pad, "все ок" (device, button bits, Guide/Share arrive). Remaining
acceptance remnant: virshot itself with a pad bind on the Windows host
(docs/spec.md §9). Task archived by user confirmation.
