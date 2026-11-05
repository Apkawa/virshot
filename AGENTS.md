# VIRShot (package `virshot`) — project conventions

Ultra-lightweight gamepad screenshot utility for Windows (cross-built from Linux/WSL).
Product requirements: `docs/spec.md`. Task process artifacts: `.dumbspec/`.

## Code structure

- **One module = one file.** Each module lives in its own file under `src/`
  (`src/main.rs` only wires things together: CLI entry, event loop, module wiring).
  A file declares exactly one module; the tests for that module follow the placement
  rules in **Testing** — never into an unrelated file.

## Formatting & pre-commit

- **Do not hand-format code.** Formatting is `cargo fmt`'s job; run it instead of
  adjusting whitespace by hand (running it is not strictly mandatory, but any
  formatting drift belongs to `cargo fmt`, not to the author).
- **Iteration gate:** `./scripts/check.sh` — `check + test + clippy -D warnings`,
  no formatting. Run it after each batch of edits; do not run `cargo fmt` after
  every edit.
- **Formatting-aware gate:** `./scripts/check_full.sh` — `check.sh`, then
  `cargo fmt` (rewrites files) and `cargo fmt --check`. Run it before committing
  and at task/stage boundaries, not on every iteration.
- **Git hooks run the pre-commit gate:**
  `./scripts/check.sh && ./cargo.sh fmt && git add -u . && ./cargo.sh fmt --check`
  (wired via `rusty-hook` in `.rusty-hook.toml`; the chain must pass before a commit
  lands). Formatting happens at commit time only: day-to-day iteration uses
  `./scripts/check.sh`, so files do not churn between edit batches.
- **Commit messages follow Conventional Commits** (`type(scope): description`,
  e.g. `feat(poc): ...`). No linter enforces this yet — keep it by convention.

## Testing

Placement follows the **visibility of the code under test**, never convenience.

- **Public API → integration tests:** `crates/<crate>/tests/<area>.rs` for library
  crates, `tests/unit/<area>.rs` for the root crate. Such a test pins the contract,
  not the implementation. `tests/unit/` needs a `lib` target on the root package;
  while `virshot` is bin-only nothing can import it, so its unit tests stay in the
  sibling files below.
- **Private / `pub(crate)` internals → sibling `<module>.tests.rs`**, attached at the
  **end** of the module file: `use super::*` reaches the private items, and
  `cfg(test)` keeps the file out of the shipped binary.

  ```rust
  #[cfg(test)]
  #[path = "capture.tests.rs"]
  mod tests;
  ```

  An inline `#[cfg(test)] mod tests { … }` is allowed only for a handful of trivial
  assertions.
- **E2E / regression tests → `tests/e2e/`** (`tests/e2e/cli_headless.rs`). They drive
  the built binary, not the crate API, and share no fixtures with unit tests.
- **Cargo target rule:** a directory under `tests/` becomes a test target only through
  its `main.rs`; a plain `.rs` file directly under `tests/` or `crates/*/tests/` is a
  target on its own. So a directory needs `main.rs` declaring one module per area:

  ```rust
  mod cli_headless;
  ```

- **One test file per module**; fixtures stay inside it, no shared test-harness crate.

- **Unit tests: TDD.** Failing test first, implement to green, refactor. Pure logic
  only — filename templates, sanitization, config parsing/defaults, path validation.
  No real gamepad, no real screen capture; time and randomness are injected (see
  *Rust Best Practices*), so assertions are deterministic.
- **E2E tests: no TDD.** They lock functionality that already shipped: run the built
  binary headless (`virshot shot`), assert a real file is written and valid. Never
  designed up front.
- **Naming:** `<unit>_<scenario>_<expectation>`.

- **Linux gate:** `./scripts/check.sh` runs `cargo test` on the root package only.
  Crate tests: `./cargo.sh test -p overlay -p toast`, standalone
  `cd crates/<crate> && ../../cargo.sh test`, or `./cargo.sh test --workspace` at
  task/stage boundaries.
- **Windows gate:** `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` compiles
  the test binaries; execution happens on Windows hosts, including `crates/gamepad`
  behind `#[cfg(windows)]`.

## Safety

- **`unsafe` is forbidden** in project code: `#![forbid(unsafe_code)]` at the
  crate root. Third-party crates may contain unsafe internally; that is outside
  this rule.
- **Documented exception:** `crates/overlay` uses `#![deny(unsafe_code)]` at the
  crate root with a module-scoped `#![allow(unsafe_code)]` confined to
  `crates/overlay/src/gl_boot.rs` — glutin/glutin-winit/glow expose context and
  surface creation only as `unsafe fn` (no safe wrapper exists, and a nested
  `allow` cannot relax `forbid`); every `unsafe` there is direct FFI into the
  platform GL API, the same shape `crates/gamepad` uses. `crates/toast` keeps
  `#![forbid(unsafe_code)]`, and the root `virshot` keeps `#![forbid(unsafe_code)]`.

## Build & environment

- Rust stable, edition 2024. Windows build: `cargo build --release --target x86_64-pc-windows-gnu`
  (MinGW-w64 linker, configured in `.cargo/config.toml`).
- In this sandbox, cargo caches must stay in the repo. Use the wrapper `./cargo.sh`
  (sets `CARGO_HOME=./.cache/cargo` and forwards all args to cargo); global
  `~/.cargo` is read-only, so bare `cargo` fails in the sandbox.
- Temporary/scratch artifacts (screenshots, harnesses, logs) go only in `./.tmp/`;
  caches only in `./.cache/`; both are git-ignored, along with `target/`.
- **Thread model:** exactly one winit `EventLoop` per process (winit 0.30 global
  guard), so the overlay thread backend is used only in standalone processes and
  examples; host apps (virshot) paint overlays via `OverlayHost` on eframe's loop,
  and the toast timer thread is plain `std` (`recv_timeout`, no winit).
- Gamepad trigger ships on Windows (task `2026-11-03_gamepad-integration`,
  2026-11-04): the trigger is the global keyboard hotkey **OR** a pad button —
  both sources fire through one shared ~250 ms debounce gate
  (`bindings::try_fire_shared`). Windows backend: `crates/gamepad` — a
  hand-written COM shim over the `gameinput.dll` exports (zero crates.io deps,
  no `windows` crate added), mechanics unchanged from the PoC: focus policy
  before registration, v3→v2→v0 negotiation, `Dispatch(5000µs)` pump on the
  dedicated thread + 50 ms button **STATE-diff** poll (not timestamp diff);
  virshot computes the 0→1 DOWN edges (`bits & !previous`) itself, Guide/Share
  arrive as system-button edges, button labels come from the device-info block
  (`gamepad::button_label`) — the vendored header has **no** `GetButtonLabel`
  slot on `IGameInputDevice`. Pad notation in config `pad`: canonical GameInput
  names, `unknown:<N>` (raw index), `Guide`, `Share`; the UI pad field captures
  a press ("press a button") and writes that notation, cancel = click the field
  again (Esc cancels keyboard capture only); any active capture suppresses pad
  shots. Linux stays **out of scope**: `gilrs` is not added — hardware finding:
  the gilrs `wgi` backend gates reads on the controller timestamp and generic HID
  pads report a constant timestamp, so presses never arrive (hardware-verified
  on user's pad vendor=1118 product=2835). Deferred: Windows hardware acceptance
  run (Share/Guide behavior on the real pad — docs/spec.md §9). `hidpad-poc/`
  stays as the historical PoC; the library duplicates it deliberately.

## Dependencies

- **Language:** Rust (latest stable, rust version 1.99.1).
- **UI and overlay:** the settings window is `eframe` + `egui`; overlay and toast
  live in path-dep crates (task `2026-11-04_overlay-toast`, 2026-11-04):
  `crates/overlay` — low-level overlay windows, a winit/glutin/`glow` thread
  backend (`src/gl_boot.rs` GL bootstrap, transparent / decoration-less /
  always-on-top / content-protected / click-through) plus the host backend
  `OverlayHost`, which paints overlays as egui multi-viewports on the host loop;
  `crates/toast` — toast rules on a plain `std` timer thread driving a
  `ToastSink` over overlay handles, with the non-blocking `ToastHandle` facade.
  Root manifest path deps `overlay = { path = "crates/overlay" }` and
  `toast = { path = "crates/toast" }` (all platforms). Stack:
  `egui`/`eframe`/`egui-winit`/`egui_glow`/`winit`/`glow`/`glutin`/`glutin-winit`/
  `image`/`thiserror` — all versions are already locked in `Cargo.lock` via
  eframe's transitive deps and were approved by the user during task
  `2026-11-04_overlay-toast`; **no new crates.io packages** were added beyond
  that approved set.
- **Gamepad:** shipped on Windows (2026-11-04, task
  `2026-11-03_gamepad-integration`) — path dep
  `gamepad = { path = "crates/gamepad" }` under
  `[target."cfg(windows)".dependencies]`; **zero crates.io deps** — the backend is
  a hand-written `gameinput.dll` COM shim, so the earlier `windows` crate plan
  (`RawGameController`) was superseded and **no new crates.io dependency was
  added** (nothing needed approval). **Workspace layout:** the root manifest
  declares `[workspace] members = [".", "crates/overlay", "crates/toast",
  "crates/gamepad"]` with `exclude = ["hidpad-poc"]` (gamepad made an explicit
  member by the user on 2026-11-04; cargo also auto-adds path-dep members —
  `exclude` cannot veto a path dependency), so `crates/gamepad` **is**
  a workspace member (`./cargo.sh clippy --workspace --all-targets` compiles
  it); it stays standalone-buildable from its own directory as before
  (`cd crates/gamepad && ../../cargo.sh test`). `hidpad-poc` gained an empty
  `[workspace]` table in its manifest so its standalone build keeps working
  under the new root workspace. The
  `windows` crate present in `Cargo.lock` is transitive via **`xcap` 0.9.8** —
  winit uses `windows-sys`, not `windows`. Linux `gilrs` stays out of scope (see
  Build & environment for the hardware finding); a Linux backend, if ever added,
  needs explicit user approval like any other new dep.
- **Screen capture:** `xcap`.
- **Global keyboard hotkeys:** `global-hotkey` (system-wide hotkey registration;
  crate chosen by user research, approved 2026-11-03; v0.9.0 with **default
  features** — the optional `serde`/`tracing` features were never needed, the
  wiring shipped without them). Manager created and re-registered on the main
  thread (Windows `WM_HOTKEY` affinity with the winit pump); events drained in
  the eframe logic pass; registrations are paused/restored around keyboard
  bind-by-press capture. The pad half of the OR-fire lives in `src/gamepad.rs`
  over the same shared debounce gate.
- **System tray:** `tray-icon` (`default-features = false, features = ["ksni"]` —
  pure-Rust SNI/D-Bus backend; the default `libappindicator` backend would need GTK
  native libs on Linux). Integration with the winit/eframe event loop: solved —
  tray events are handled in the main-thread eframe pass (`gui::run`).
- **Folder picker:** `rfd` (spec §4.4 Browse button; approved by the user
  2026-11-03; v0.17.2, default features `xdg-portal` + `wayland` — the blocking
  `FileDialog::new().set_directory(..).pick_folder()` needs no async/tokio
  feature; Windows native common controls, Linux XDG portal with a zenity
  fallback; a missing backend returns `None`, never panics). No new native
  build package: libdbus is `dlopen`d at runtime and absent backends degrade to
  `None`.
- **Configuration:** `serde` + `toml`.
- **Error types:** `thiserror` (per-module error enums; typed errors over stringly `Err(String)`).
- **Logging:** `tracing` + `tracing-subscriber` (file appender).
- **CLI:** `clap`.

Keep the dependency set minimal: add crates only when an MVP feature requires it,
and pin the backend/features a crate needs. Native build deps: Windows-gnu needs
MinGW-w64 only; the Linux `xcap` chain needs `libudev-dev`, `libwayland-dev`,
`libpipewire-0.3-dev`, `libclang-dev`, `libegl1-mesa-dev`, `pkg-config`.

- **New dependencies require explicit user confirmation.** Adding a dependency
  that is not in the list above — a new crate, a new system/native package, or a
  new toolchain target — must be proposed with rationale and approved by the
  user before it is added or installed.
- **Crate API knowledge** (research stage, and mid-implementation when an API is
  not obvious): `./scripts/cratedoc.sh <crate>::<item>` renders the locked
  version's docs to markdown with actionable links — item links become
  `./scripts/cratedoc.sh` calls, `Source` links point into
  `.cache/cargo/registry/src/...:<lines>` (rustdoc JSON needs nightly; docs.rs
  serves no markdown); `./cargo.sh doc -p <crate>` for the full page set; crate
  README via crates.io API `content` or GitHub raw. Verify by building — not by
  memory.

## Rust Best Practices

- **Errors are values, not panics.** Functions that can fail return `Result`;
  their doc comments state the failure mode. No `unwrap()`/`expect()` outside
  tests and startup init; the app loop logs failures and keeps running.
- **Inject time and randomness** into functions that produce filenames/timestamps
  (pass `DateTime` in, don't call `Local::now()` deep inside) — keeps unit tests
  deterministic.
- **Keep module interfaces tight**: `pub(crate)` by default, `pub` only for what
  another module actually consumes. No glob imports except crate preludes.
- **Prefer references over clones**; avoid `.clone()` in hot paths (capture loop,
  event loop).
- **Naming**: `snake_case` functions, `PascalCase` types; tests named
  `<unit>_<scenario>_<expectation>`.
- **Thread boundary**: capture runs on a spawned thread; communicate by owned
  values or channels, not by shared `Mutex` state.

### Ownership & Cloning

- **Justify every `.clone()`.** Prefer move → borrow → clone, in that order.
  Clone only when the original must remain valid and borrowing is impossible
  (e.g., a `'static` bound, transfer to another thread/task, or shared ownership
  in multiple places).
- **Do not derive `Clone` by default.** Derive it only for value types and cheap
  handles. Never derive `Clone` on types holding unique resources (connections,
  file descriptors, mutable state).
- **Never call `.clone()` twice on the same value** in a single scope, and never
  clone a value that is dropped shortly after.

### `Arc` and Shared State
- **`Arc` is for shared ownership, not for making clones cheap.** Use it when a
  value must be owned in multiple places simultaneously, especially under a
  `'static` requirement (`tokio::spawn`, `std::thread::spawn`, callbacks).
- **Do not wrap already-shared types in `Arc`.** `Sender<T>`, `Receiver<T>`,
  `Arc<T>`, and `Rc<T>` already clone in O(1).
- **Prefer `Arc<Inner>` over `Arc<WholeStruct>`** unless the struct is logically
  a single unit.
- **Prefer channels over `Arc<Mutex<T>>`** for communication between tasks. Use a
  mutex only for genuinely shared mutable state.
- Small values (`u64`, short `String`, `Option<Handle>`) do not benefit from
  `Arc`; clone them directly if needed.

### Signatures
- **Take `&T` when the callee only reads.** Take `T` by value only when ownership
  is actually transferred or consumed.
- If a function owns one part of a value and borrows another, **destructure it at
  the start** rather than cloning.

### Struct Design
- **Separate concerns in command/config structs.** Do not mix "what to do" with
  "where to report" — a notification handle belongs in a separate parameter, not
  inside the request struct.
- Apply `#[derive(Clone)]` only where a real consumer exists in the codebase.

### Errors & Safety
- Return `Result<T, E>` with a project-defined `Error` type. Do not panic in
  library or application code paths that can fail.
- Callers are responsible for logging; library functions must not swallow errors
  silently.

## Scope guard

- MVP scope is defined in `docs/spec.md`; anything not listed there is out of scope
  until explicitly added by the user.

## Design Principles

### 1. KISS / YAGNI
Choose the simplest solution that satisfies requirements and tests. Do not add speculative extensibility, configuration, interfaces, factories, or generic abstractions. Do not duplicate business rules or complex logic; however, tolerate local duplication rather than introduce a wrong abstraction.

### 2. Single Semantics per Module (SRP)
One module has one responsibility, expressed by its name. Do not mix unrelated concerns.
