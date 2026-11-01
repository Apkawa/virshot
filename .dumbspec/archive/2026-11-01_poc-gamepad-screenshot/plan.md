# 2026-11-01_poc-gamepad-screenshot: implementation plan (POC: gamepad + screenshot)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

Note (POC reality): the POC is a console app driving real gamepad/screen I/O;
"red test" for stages 3–4 means a failing automated check (unit test on the pure
filename helper / Linux capture integration), not a gamepad-driven test —
gamepad acceptance is manual on Windows (Stage 5, user potests the .exe).

---

## Stage 0 — Research + spec + plan
- [x] draft.md (raw input + user refinements)
- [x] research.md (environment facts, crate facts, verdict)
- [x] spec.md v1 (scope, approach, constraints, open questions)
- [x] plan.md

**Criterion:** all four artifacts exist; spec v1 has Created + Changes; verdict says windows-gnu needs only MinGW-w64.
**Commit:** `docs(spec): POC gamepad screenshot — draft, research, spec v1, plan`

## Stage 1 — System dependencies (user runs apt)
- [x] User: `sudo apt update && sudo apt install -y mingw-w64` (done: `/usr/bin/x86_64-w64-mingw32-gcc` present)
- [~] User: `sudo apt install -y libudev-dev libwayland-dev libpipewire-0.3-dev libclang-dev` — libudev/wayland/pipewire pkg-config OK; **new gap found in Stage 2: `egl.pc` missing (khronos-egl via xcap linux chain) → also needs egl dev package**
- [x] Verify in sandbox: `which x86_64-w64-mingw32-gcc` + pkg-config .pc files present

**Criterion:** `x86_64-w64-mingw32-gcc --version` works; `pkg-config --exists libudev libwayland-client libpipewire-0.3` all succeed.
**Commit:** N/A (no repo change; verification recorded here)

## Stage 2 — Rust project scaffold + build config
- [x] `Cargo.toml`: name `vibe-shot`, edition 2024, deps gilrs 0.11.2 (default-features=false, features=["xinput"]), xcap 0.9.8, chrono 0.4.45
- [x] `.cargo/config.toml`: `[target.x86_64-pc-windows-gnu] linker = "x86_64-w64-mingw32-gcc"`
- [x] `.gitignore`: add `target/`, `.cache/`
- [x] Minimal `src/main.rs` placeholder; `cargo check --target x86_64-pc-windows-gnu` OK (linux blocked by missing egl dev pkg, env-only)
- [x] Red: unit tests for the filename helper fail → green: helpers + 3 tests pass (harness evidence; windows-gnu test exe also links)

**Criterion:** `cargo test` passes; `cargo check --target x86_64-pc-windows-gnu` succeeds; deps locked in Cargo.lock.
**Commit:** `feat(poc): rust scaffold, cross-build config, filename helper + tests`

## Stage 3 — Gamepad polling loop (console output)
- [x] `gilrs` loop: print connected gamepad names on connect/disconnect events (INFO)
- [x] Print every `ButtonPressed` (button name + gamepad name) to stdout; poll sleep 10ms
- [x] Graceful behavior when no gamepad attached (no panic, keep polling for plug-in)

**Criterion:** Linux mirror-harness run prints listening line, stays alive (exit 124, no panic); windows-gnu debug build links; tests green (7 passed in harness). Note: repo build uses gilrs xinput-only, so Linux host detects no pads — real pad acceptance is on Windows.
**Commit:** `feat(poc): gilrs polling loop with console button output`

## Stage 4 — Screenshot on ANY button press
- [x] On any `ButtonPressed` → spawn thread: `xcap::Monitor::all()` → primary monitor `capture_image()` → save PNG to `./.tmp/screenshots/screenshot_<ts>.png`
- [x] Capture logic proven via harness (linux repo build blocked by egl env): 8 tests green, PNG artifacts with valid magic bytes, error paths return Err without panic
- [x] Failure path: capture/save error printed to stderr, no panic (verified: no unwrap in capture code)

**Criterion:** capture path produces a valid PNG file and clean Err on failures (harness evidence); windows-gnu builds clean, no warnings. Linux forced-capture rerun deferred until `libegl-dev` lands.
**Commit:** `feat(poc): screenshot capture on gamepad button press via xcap`

## Stage 5 — Windows build + handoff
- [x] `cargo build --release --target x86_64-pc-windows-gnu` → `target/x86_64-pc-windows-gnu/release/vibe-shot.exe` (883,712 B ≈ 0.84 MB, PE32+ console x86-64, MZ header)
- [x] Smoke: WSL interop unavailable in this instance (no cmd.exe/binfmt/wine) — accepted deviation; first real run is on Windows
- [x] Handoff checklist to user: `.tmp/release/vibe-shot.exe` (md5-identical copy) + `.tmp/release/HANDOFF.md`

**Criterion:** windows-gnu release exe exists and is a valid PE binary; handoff artifact + checklist in place; user potests on Windows.
**Commit:** `build(poc): windows-gnu release build verified`
