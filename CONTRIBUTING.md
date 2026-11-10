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
SKIP_E2E=1 ./scripts/check.sh                         # same gate, e2e compiled but not run (pre-commit uses this)
./scripts/check_full.sh                               # gate above + cargo fmt + fmt --check
./cargo.sh test --test e2e                            # e2e only: drives the built binary
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

## CI (GitHub Actions)

Both workflows invoke plain `cargo`; the repo wrapper is a sandbox convenience,
not a CI step.

### `ci.yml`

Triggers: `push` to `master`, `pull_request`, `workflow_dispatch`.

- **`linux`** (`ubuntu-latest`): installs the native dev packages above first
  (`xcap`/GL chain — without `libgbm-dev` the link fails with
  `rust-lld: error: unable to find library -lgbm`), then runs
  `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`,
  `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- **`windows`** (`windows-latest`): no extra packages; runs
  `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`.

### `release.yml`

Triggers: tag push `v*`, `push` to `master`, `workflow_dispatch`; the job
publishes with `contents: write`.

- **`build`** matrix: `x86_64-pc-windows-msvc` on `windows-latest` and
  `x86_64-unknown-linux-gnu` on `ubuntu-latest` (Linux installs the native dev
  packages above). Each binary is uploaded as an artifact: `binary-win-x64` /
  `binary-linux-x64`. The shipped Windows build is MSVC — the local gnu
  cross-build above is the dev path only.
- **`package`** (`ubuntu-latest`, `needs: build`): downloads both artifacts,
  installs git-cliff as a dev **tool** (`cargo install git-cliff --version 2.14.2
  --no-default-features` — not a crate dependency), writes the release body to
  `dist/notes.md` (`git-cliff --latest` for tag releases, `git-cliff --unreleased`
  for nightly), packs the archives with `scripts/package-release.sh`, and
  publishes via `gh`.

Artifact names (two targets in scope; `aarch64-pc-windows-msvc` / `win-arm64`
is deferred as a separate future task):

| build | tag | archives |
| --- | --- | --- |
| release | `v<version>` | `virshot-v<version>-win-x64.zip`, `virshot-v<version>-linux-x64.tar.gz` |
| nightly (any `master` commit) | `nightly` | `virshot-v<version>-nightly-<shortsha>-win-x64.zip`, `virshot-v<version>-nightly-<shortsha>-linux-x64.tar.gz` |

Archives hold the bare binary only (`virshot.exe` / `virshot`). The `nightly`
prerelease is updated in place: old assets are deleted and the target commit is
moved.

Alongside the archives every release ships `dist/release.yaml`:

```yaml
version: 0.1.0
releaseDate: '2026-11-09T18:00:00.000Z'  # ISO-8601, ms precision, UTC
commit_sha: 6afa08e                      # short sha
files:
  - url: https://github.com/<owner>/<repo>/releases/download/<tag>/<file>
    type: x86_64-pc-windows-msvc         # target triple
    sha512: <base64 of the raw sha512 digest>
    size: <archive bytes>
```

`scripts/release.sh` stays local: ZeroVer `changelog` / `minor` / `patch`
(`major` is forbidden) regenerates `CHANGELOG.md`, bumps `Cargo.toml`, commits
and creates the annotated tag. Pushing and publishing happen in Actions, never
from the script.
