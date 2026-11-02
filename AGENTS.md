# VIRShot (package `virshot`) — project conventions

Ultra-lightweight gamepad screenshot utility for Windows (cross-built from Linux/WSL).
Product requirements: `docs/spec.md`. Task process artifacts: `.dumbspec/`.

## Code structure

- **One module = one file.** Each module lives in its own file under `src/`
  (`src/main.rs` only wires things together: CLI entry, event loop, module wiring).
  A file declares exactly one module; tests for a module go into that file's
  `#[cfg(test)] mod tests` or a sibling `src/<module>.tests`-style file — never
  into an unrelated file.

## Formatting & pre-commit

- **Do not hand-format code.** Formatting is `cargo fmt`'s job; run it instead of
  adjusting whitespace by hand (running it is not strictly mandatory, but any
  formatting drift belongs to `cargo fmt`, not to the author).
- **Git hooks run the pre-commit gate:**
  `cargo check && cargo test && cargo fmt && git add -u . && cargo clippy -- -D warnings`
  (wired via `rusty-hook`; the chain must pass before a commit lands).
- **Commit messages follow Conventional Commits** (`type(scope): description`,
  e.g. `feat(poc): ...`). No linter enforces this yet — keep it by convention.

## Testing

- **Unit tests: TDD.** Write the failing test first, implement to green, refactor.
  Unit tests cover pure logic only: filename templates, sanitization, config
  parsing/defaults, path validation. No real gamepad, no real screen capture.
- **E2E tests: no TDD.** They fix already-implemented functionality as regression
  tests: run the built binary headless (`virshot shot`), assert a real file is
  written and valid. Place them in `tests/`. Do not design E2E up front; record
  functionality once it ships.
- All tests must pass on both targets: `cargo test` (linux) and
  `cargo test --no-run --target x86_64-pc-windows-gnu` (windows-gnu compiles;
  execution of Windows test binaries happens on Windows hosts).

## Safety

- **`unsafe` is forbidden** in project code: `#![forbid(unsafe_code)]` at the
  crate root. Third-party crates may contain unsafe internally; that is outside
  this rule.

## Build & environment

- Rust stable, edition 2024. Windows build: `cargo build --release --target x86_64-pc-windows-gnu`
  (MinGW-w64 linker, configured in `.cargo/config.toml`).
- In this sandbox, cargo caches must stay in the repo. Use the wrapper `./cargo.sh`
  (sets `CARGO_HOME=./.cache/cargo` and forwards all args to cargo); global
  `~/.cargo` is read-only, so bare `cargo` fails in the sandbox.
- Temporary/scratch artifacts (screenshots, harnesses, logs) go only in `./.tmp/`;
  caches only in `./.cache/`; both are git-ignored, along with `target/`.
- `gilrs` must be built with `default-features = false, features = ["xinput"]`
  (default WGI backend delivers no events to a console/tray process on Windows).

## Dependencies

- **Language:** Rust (latest stable, rust version 1.99.1).
- **UI and overlay:** `eframe` + `egui` (multi-viewport for the settings window and overlay).
- **Gamepad:** `gilrs` (XInput / DirectInput; in this project built with
  `default-features = false, features = ["xinput"]`).
- **Screen capture:** `xcap`.
- **Global keyboard hotkeys:** TODO: research
- **System tray:** `tray-icon` (`default-features = false, features = ["ksni"]` —
  pure-Rust SNI/D-Bus backend; the default `libappindicator` backend would need GTK
  native libs on Linux). Integration with the winit/eframe event loop: still to solve.
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

## Best practices

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

## Scope guard

- MVP scope is defined in `docs/spec.md`; anything not listed there is out of scope
  until explicitly added by the user.
- **Git hooks live in tracked `.githooks/`** (`git config core.hooksPath .githooks`).
  `rusty-hook`'s dev-dependency build script regenerates `.git/hooks/*` on every cargo
  build, and its auto-install writes to the read-only global `~/.cargo`; the `.githooks`
  copy exports `CARGO_HOME=./.cache/cargo` and puts `.cache/cargo/bin` (rusty-hook CLI)
  on PATH. Run `git config core.hooksPath .githooks` once per clone.
