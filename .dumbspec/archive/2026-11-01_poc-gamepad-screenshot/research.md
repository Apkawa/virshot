# Research: cross-built Windows binary (x86_64-pc-windows-gnu) — gamepad screenshot POC

Sampled 2026-11-01 (host: Ubuntu 24.04 noble under WSL). Facts only.

## A. Local environment

- **Toolchain**: `rustc 1.99.1`, `cargo 1.99.1`, `rustup 1.26.0` (`/usr/bin/rustup`), active toolchain `stable-x86_64-unknown-linux-gnu`.
- **Targets installed**: `x86_64-pc-windows-gnu` **yes** (already installed, std present) and `x86_64-unknown-linux-gnu`.
- **MinGW-w64**: **NOT installed**. `x86_64-w64-mingw32-gcc` and `x86_64-w64-mingw32-ld` not on PATH; no `mingw*` dpkg entries. apt candidate: `mingw-w64 11.0.1-3build1` (Ubuntu noble/universe, mirror.yandex.ru).
- **Linker situation (measured)**:
  - `rustc --target x86_64-pc-windows-gnu` on a stub `fn main(){}` → compiles, then `error: linker 'x86_64-w64-mingw32-gcc' not found`. So rustc's *default* linker name for windows-gnu is already `x86_64-w64-mingw32-gcc`.
  - `-C linker=rust-lld` also fails: `lld: error: unable to find library -lkernel32 / -lntdll / -lws2_32 / -lmsvcrt / -lmingwex ...` → Windows import libs (.dll.a) must come from a MinGW-w64 sysroot; lld alone is not enough.
  - Rust's bundled mingw runtime IS present in the sysroot (`lib/rustlib/x86_64-pc-windows-gnu/lib/self-contained/crt2.o`, `dllcrt2.o`, `libstd.dll.a`, `std.dll`). No separate `rust-mingw` rustup component in rustup 1.26 — it ships inside `rust-std`. **No `rustup component add rust-mingw` needed/possible.**
- **Repo state**: only `docs/spec.md` (+ `.git`, `.dumbspec/`, `.tmp/`). No `Cargo.toml`, no `src/`, no `.cargo/config.toml`. `.gitignore` content: `.tmp` (single line).
- **Network**: crates.io works. `cargo search gilrs` OK; crates.io API OK; index update + lock (269 pkgs) + download work — but `~/.cargo/registry/cache` is **read-only in this sandbox**; builds here must use a workspace cache: `CARGO_HOME=/home/apkawa/code/vibe-shot/.cache/cargo cargo ...` (verified working).
- **sudo**: unavailable in this sandbox (`no new privileges` flag) → package installs must be run by the user in a normal shell.
- **Linux-build deps missing**: `pkg-config` present, but `libudev-dev`, `libwayland-dev`, `libpipewire-0.3-dev`, `clang/libclang` NOT installed (runtime libs are present: `libudev.so.1`, `libxcb*`, `libpipewire-0.3.so.0`).

### Compile probes (scratch crate `.tmp/resprobe/`, deps `gilrs 0.11.2` xinput, `xcap 0.9.8`, `chrono 0.4.45`)
- `cargo check --target x86_64-pc-windows-gnu`: **SUCCESS** (51 s). All deps compile: gilrs-core 0.6.8, rusty-xinput 1.3.0, windows 0.62.2, image 0.25.10 (+ png 0.18.1), chrono 0.4.45. Only the final link step is blocked by missing MinGW. API usage (`Gilrs::new`, `EventType::ButtonPressed`, `Monitor::all`, `capture_image().save("*.png")`, `chrono::Local::now().format`) type-checks.
- `cargo check --target x86_64-unknown-linux-gnu`: **FAILS** — `wayland-sys` build.rs requires `wayland-client.pc` (via xcap → libwayshot-xcap → `wayland-backend/client_system`); additionally `libudev-sys` probes `libudev` via pkg-config and `pipewire-sys` probes `libpipewire` + bindgen (needs libclang). Linux build needs: `sudo apt install libudev-dev libwayland-dev libpipewire-0.3-dev libclang-dev`.

## B. Crates (latest on crates.io, 2026-11-01)

| crate | version | notes |
|---|---|---|
| gilrs | **0.11.2** (gilrs-core 0.6.8) | repo gitlab.com/gilrs-project/gilrs |
| xcap | **0.9.8** | repo github.com/nashaofu/xcap, edition 2024 (needs rustc ≥ 1.85 — ok) |
| image | **0.25.10** | pulled by xcap |
| chrono | **0.4.45** | trivial |

### gilrs
- Windows backends via Cargo features: default `wgi` = Windows Gaming Input (`windows` crate, `Gaming_Input`); alternative `xinput` = `rusty-xinput` 1.3.0 + `winapi` (loads `xinput1_x.dll` at runtime via LoadLibrary → MinGW-friendly, no SDK headers).
- **Key caveat**: WGI "requires an in-focus window associated with the process to receive events". For a **console** POC use `gilrs = { version = "0.11.2", default-features = false, features = ["xinput"] }`.
- Cross-compiling for windows-gnu: no crate-specific linker requirements; compiles cleanly here (verified).
- API for printing presses: `let mut g = Gilrs::new().unwrap();` — enumerate: `for (_id, gp) in g.gamepads() { gp.name() }` — events: `while let Some(Event { id, event, time, .. }) = g.next_event()` — match `EventType::ButtonPressed(button, _data)`; `Button` variants: `North, South, East, West, NorthEast, SouthEast, SouthWest, NorthWest, LeftShift, RightShift, LeftTrigger, RightTrigger, Select, Start, Mode, ThumbLeft, ThumbRight`; state: `g.gamepad(id).is_pressed(Button::South)`.

### xcap
- Windows backend: `windows` crate **0.62** (Win32 Gdi/Dwm/Devices_Display/Dxgi/Direct3D… + `widestring`); pure-Rust bindings, **no C/native deps beyond MinGW import libs** (gdi32/user32/dxgi .dll.a — MinGW-w64 ships them). Requires Windows ≥ 8.1.
- `image` dep is `default-features = false, features = ["png"]` → **PNG saving enabled by default** (`DynamicImage::save("…png")` works; xcap feature `image = ["image/default"]` exists if broader formats wanted). No extra feature flags needed for PNG.
- API: `xcap::Monitor::all() -> Vec<Monitor>`; `monitor.capture_image() -> image::DynamicImage`; also `is_primary()`, `width()/height()`, `friendly_name()`. docs.rs build of 0.9.8 fails (no rendered docs; README examples are authoritative).
- Linux backend pulls xcb/wayland/pipewire/zbus unconditionally → the dev-package requirements listed above.

### windows-gnu cross-build config
- Precompiled std ships for windows-gnu; cross-compiling from Linux works once a MinGW-w64 C toolchain is installed (rustc book: [platform support](https://doc.rust-lang.org/rustc/platform-support/windows-gnu.html)).
- Recommended explicit config (default linker name already matches, so optional):
  ```toml
  # .cargo/config.toml
  [target.x86_64-pc-windows-gnu]
  linker = "x86_64-w64-mingw32-gcc"
  ```

## Verdict

**Build windows-gnu right now? NO — one missing piece: MinGW-w64.** Required (run by user, sudo not available in sandbox):

```bash
sudo apt update && sudo apt install -y mingw-w64   # provides x86_64-w64-mingw32-gcc + import libs
cargo build --target x86_64-pc-windows-gnu         # after Cargo.toml exists
```
Everything else (rust target, crates.io, dependency compile) is already verified working. In-sandbox builds need `CARGO_HOME=/home/apkawa/code/vibe-shot/.cache/cargo`.

**Linux build** additionally needs: `sudo apt install -y libudev-dev libwayland-dev libpipewire-0.3-dev libclang-dev`.

### Risks
1. apt `mingw-w64` is 11.0.1 vs rustc book's oldest-tested toolchain (mingw-w64 12.0, binutils 2.44, gcc 14.2) — usually fine, but if `ld` errors appear, switch to a newer MinGW (MSYS2/official builds).
2. gilrs default `wgi` gets **no events in a console app** → must use `xinput` feature (verified it compiles for windows-gnu).
3. xcap windows-gnu **link** step not yet verified end-to-end (needs MinGW); compile step verified OK.
4. docs.rs docs for xcap 0.9.8 are broken; rely on README/source.
5. Running the produced `.exe`: WSL executes Windows binaries directly; gamepad on Windows host visible to the Windows binary.
