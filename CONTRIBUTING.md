# Contributing — build cheat-sheet

## Prerequisites

### Linux build host (native dev packages)

`xcap`'s Linux backend links against native libraries; without them the build
fails (e.g. `rust-lld: error: unable to find library -lgbm`, or
`The system library 'egl' required by crate 'khronos-egl' was not found`):

```bash
sudo apt install -y pkg-config libegl1-mesa-dev libgbm-dev \
  libudev-dev libwayland-dev libpipewire-0.3-dev libclang-dev
```

### Windows cross-build (from Linux/WSL)

Only the MinGW-w64 toolchain (no native dev libs — `xcap` on Windows is pure Rust):

```bash
sudo apt install -y mingw-w64
```

`x86_64-pc-windows-gnu` linker is pinned in `.cargo/config.toml`.

## Commands

```bash
cargo test                                            # linux: build + unit tests
cargo build --release --target x86_64-pc-windows-gnu  # windows release exe
cargo test --no-run --target x86_64-pc-windows-gnu    # windows: compile tests (run on Windows)
./scripts/check.sh                                    # gate: check + test + clippy, no formatting
./scripts/check_full.sh                               # gate above + cargo fmt + fmt --check
./scripts/cratedoc.sh xcap::Monitor                   # crate API docs as markdown
```

The Windows release exe lands in
`target/x86_64-pc-windows-gnu/release/virshot.exe` (~0.84 MB, `opt-level="z"`,
`lto="thin"`, `strip=true`).

## Sandbox note

In the agent sandbox the global `~/.cargo` is read-only; use the repo wrapper
instead of bare cargo:

```bash
./cargo.sh test    # sets CARGO_HOME=./.cache/cargo and forwards args to cargo
```

## Git hooks (one-time setup per clone)

The pre-commit gate is wired via `rusty-hook`, and the hook scripts live in the
tracked `.githooks/` directory — plain `git` only reads `.git/hooks`, so hooks do
not work out of the box. Point git at them once per clone:

```bash
git config core.hooksPath .githooks
```

The gate chain (from `.rusty-hook.toml`) must pass before a commit lands:
`./cargo.sh check && ./cargo.sh test && ./cargo.sh fmt && git add -u . && ./cargo.sh clippy -- -D warnings`.

Hook scripts resolve the `rusty-hook` CLI from `PATH`; with a writable `~/.cargo`
the first hook run auto-installs it. In the sandbox (`~/.cargo` read-only) install
it into the repo-local cache first:

```bash
./cargo.sh install rusty-hook    # lands in .cache/cargo/bin, which .githooks/cli.sh puts on PATH
```

Do not edit `.git/hooks/` by hand: `rusty-hook`'s build script regenerates that
directory on every cargo build. Changes belong in `.githooks/`.

## CI (GitHub Actions)

Linux jobs must install the native dev packages above before build/test.
Windows jobs need no extra packages.
