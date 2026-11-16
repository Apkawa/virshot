# 2026-11-16_ci-optimization: research notes (English)

Collected by a research subagent (2026-11-16). Superficial context only; no solutions.

## A. Local project scan

### A.1 Workflows
- `.github/workflows/` contains **only `ci.yml`** (196 lines). `release.yml` was deleted in commit `ccc5ea9` ("chore: merge release to one ci file", 2026-11-13); release steps now live inside `ci.yml` as a `release` job.
- Docs drift: `AGENTS.md` and `CONTRIBUTING.md` still describe a separate `release.yml` / "package" job — the live workflow has a single `release` job. (Not in scope to fix unless user asks.)

### A.2 Workflow-level config (`ci.yml`)
- Triggers: `push` to `master`, `push` tags `['v*']`, `pull_request`, `workflow_dispatch`.
- `concurrency`: group `${{ github.workflow }}-${{ github.ref }}`, `cancel-in-progress: true`.
- `permissions: contents: write`.
- Workflow-level `env`: `CARGO_TERM_COLOR: always`, `RUST_BACKTRACE: 1`, `GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}`, `RELEASE_TAG: ${{ github.ref_type == 'tag' && github.ref_name || 'nightly' }}`.

### A.3 Jobs
| job | runs-on | cargo commands | notes |
|---|---|---|---|
| `linux` | ubuntu-latest | `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` | apt native dev pkgs: `pkg-config libegl1-mesa-dev libgbm-dev libudev-dev libwayland-dev libpipewire-0.3-dev libclang-dev` |
| `windows` | windows-latest | `cargo test --workspace -- --skip 'cli_headless::'`, `cargo clippy --workspace --all-targets -- -D warnings` | no extra packages |
| `build` | matrix `${{ matrix.os }}` | `rustup target add <target>`, `cargo build --release --target <target>` | `needs: [linux, windows]`, `if: github.event_name != 'pull_request'`, `fail-fast: false` |
| `release` | ubuntu-latest | none (git-cliff + scripts + `gh`) | `needs: [build]`, same `if` guard |

Build matrix (include entries): `x86_64-pc-windows-msvc` on windows-latest (key `win-x64`), `x86_64-unknown-linux-gnu` on ubuntu-latest (key `linux-x64`). Artifacts land in `target/<triple>/release/` (inside repo `./target`), uploaded via `actions/upload-artifact@v4` (`binary-win-x64`, `binary-linux-x64`).

### A.4 git-cliff in CI (`release` job)
- `actions/checkout@v4` with `fetch-depth: 0` (for git-cliff history).
- Step `Install git-cliff`: `cargo install git-cliff --version 2.14.2 --no-default-features` (this is the slow step to replace).
- Step `Generate release notes`: `git-cliff --latest -o dist/notes.md` (tag release) or `git-cliff --unreleased -o dist/notes.md` (nightly). No `--config` flag — relies on root `cliff.toml` auto-detection.
- Other steps: download-artifact ×2, `Resolve build kind` (`kind=release|nightly`), `Read version` (sed from Cargo.toml), `Package archives` via `scripts/package-release.sh`, publish via `gh`.

### A.5 Existing caching — NONE
- No `actions/cache`, no `Swatinem/rust-cache`, no `CARGO_HOME` override in workflows.
- Actions used, all pinned by floating tag (none by SHA): `actions/checkout@v4` ×4, `actions/upload-artifact@v4`, `actions/download-artifact@v4` ×2.
- No toolchain action; runners use preinstalled toolchain + `rust-toolchain.toml` (`channel = "stable"`).
- `Cargo.lock` has 0 git-cliff references (tool, not dependency).

### A.6 Scripts
- `scripts/release.sh` `resolve_cliff()`: PATH `git-cliff` first, else `$ROOT/.cache/cargo/bin/git-cliff`, else error. Invokes with `--config "$CFG"` (local changelog flow). Local install today: `./cargo.sh install git-cliff --version 2.14.2 --no-default-features` → `./.cache/cargo/bin/git-cliff`.
- `scripts/package-release.sh`: no git-cliff references; needs `zip`, `tar`, `openssl`; its `sha512_b64()` = `openssl dgst -sha512 -binary | openssl base64 -A` — project's own convention is **base64**, unlike git-cliff's published `.sha512` (**hex**).

## B. git-cliff binary releases (verified)

- Docs page lists asset naming: `git-cliff-${version}-x86_64-unknown-linux-gnu.tar.gz` and `...-musl.tar.gz`. Docs mention **PGP key `1D2D410A741137EBC544826F4A92FA17B6619297`** for signing, but do NOT document `.sha512` usage.
- v2.14.2 release assets (from expanded_assets HTML): Linux tarballs for x86_64/i686/aarch64 × {gnu,musl} each published **with `.sha512` and `.sig`**; macOS darwin tarballs with `.sha512`/`.sig`; Windows zips have **no** checksum/signature assets. No `.asc`; `.sig` is binary detached PGP (566 B for linux-gnu).
- Tarball layout: top-level dir `git-cliff-2.14.2/` containing `git-cliff` binary (+ LICENSE/README/completions/man). gnu + musl extract to the **same folder name** → extract into separate dirs or extract and move the binary.
- `.sha512` content format (verified by download + local recompute): one line `sha512sum`-style — lowercase **hex digest** + two spaces + filename:
  `26d1f7c8...f18a  git-cliff-2.14.2-x86_64-unknown-linux-gnu.tar.gz`
  `95b0c55a...29a9  git-cliff-2.14.2-x86_64-unknown-linux-musl.tar.gz`
  Independent `sha512sum` matched both. Verification shape: download `<file>.sha512`, compare digest of downloaded tarball (or `sha512sum -c`-compatible line).
- gnu vs musl (verified on binaries): gnu = dynamically linked vs glibc (`/lib64/ld-linux-x86-64.so.2`); musl = static-pie, no interpreter. Both run `git-cliff 2.14.2`. Ubuntu runners have glibc → gnu works; musl also works. Sizes: gnu tar.gz ~7.42 MB, musl ~6.91 MB.
- v2.14.2 is the newest git-cliff tag; annotated tag `v2.14.2` peels to commit `5d996de8220d26c0cb8ff329d31791570cab262f`.
- Alternative integrations documented by git-cliff: `git-cliff-action`, `setup-git-cliff`, `taiki-e/install-action` (not chosen — draft explicitly asks for direct binary download).

## C. Swatinem/rust-cache (verified)

- Latest release tag: **v2.9.3**, peeled commit SHA **`d1527c8f67ef9af357290475d4af4b0fab8f12e0`** (the zeroclaw example pins v2.9.2 at `6323deb1...`; v2.9.3 is newer). `@v2` floating tag exists.
- Inputs: `key` (extra key alongside job key), `shared-key` (stable across jobs), `cache-on-failure` (default false; post-if runs save when `success() || CACHE_ON_FAILURE=='true'`), `save-if` (default true), `add-job-id-key` (default true), `add-rust-environment-hash-key` (default true — hashes Cargo.lock, workspace Cargo.tomls, rust-toolchain*, .cargo/config.toml, env-vars), `env-vars` default `CARGO CC CFLAGS CXX CMAKE RUST`, `workspaces` default `. -> target`, `cache-bin` default true (caches `$CARGO_HOME/bin`), output `cache-hit`.
- Caches `~/.cargo` (registry, cache, git deps; NOT `registry/src`; drops pre-existing `~/.cargo/bin` files at save) and `./target` (dependency artifacts only; sets `CARGO_INCREMENTAL=0`). 10 GB total cap.
- Works on ubuntu/mac/windows runners (its own CI matrix exercises windows-latest). Workspace builds supported via `workspaces`.
- README ordering rule: toolchain selection (`rustup toolchain install`) must happen **before** the action (rustc version is part of the key). `rustup target add` is not toolchain selection; pattern is checkout → cache → cargo steps.
- Multi-job sharing: `shared-key` or `add-job-id-key: "false"`; save restricted by `save-if`.
- Project-specific: workflow env `CARGO_TERM_COLOR`/`RUST_BACKTRACE` match `CARGO`/`RUST` env-vars prefixes but values are constant → no key churn. `rust-toolchain.toml` + `.cargo/config.toml` are hashed into key. Build artifacts land under `./target/<triple>/release/` — inside the default cached `./target`.
- Interaction: if git-cliff stays a `cargo install`, its binary lands in `~/.cargo/bin` and rust-cache prunes it at save; using a downloaded binary outside `~/.cargo/bin` avoids that.

## Uncertainties
- GitHub REST API was rate-limited (403); asset inventory from HTML expanded_assets + HEAD probes; SHAs from `git ls-remote`.
- rust-cache per-target-subdir caching behavior is inferred from `./target` default, not explicitly documented.
- Docs give no gnu/musl CI-runner selection guidance (runner has glibc → gnu is safe; musl static also works).
