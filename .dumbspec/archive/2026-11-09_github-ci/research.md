# 2026-11-09_github-ci: research notes

Research = superficial context scan (facts from the repo, no design). Sources:
subagent scan report (2026-11-09), repo files, `git` state, local build probes.

## 1. Existing CI / automation

- **No `.github/` directory at all** — no workflows, no actions, no dependabot.
  No GitLab/Travis/AppVeyor config among the 155 tracked files. No Dockerfile.
- All `*.yml`/`*.yaml` hits in the tree are vendored crates in `.cache/cargo/`
  (git-ignored), not repo CI.
- Prose CI guidance already exists: `CONTRIBUTING.md:52-55` — section
  "CI (GitHub Actions)": "Linux jobs must install the native dev packages above
  before build/test. Windows jobs need no extra packages."
- Nothing in the repo generates `release.yaml` today; no checksum tooling exists.

## 2. Release tooling (already shipped, task `2026-11-09_git-cliff-release`)

- `scripts/release.sh` (90 lines): modes `changelog`, `minor`, `patch`;
  `major` is refused (ZeroVer, line 60). No push anywhere — commit + tag only.
- git-cliff lookup order: PATH `git-cliff`, else `$ROOT/.cache/cargo/bin/git-cliff`,
  else error pointing at `./cargo.sh install git-cliff --version 2.14.2
  --no-default-features`. Verified: `.cache/cargo/bin/git-cliff --version` →
  `git-cliff 2.14.2`. git-cliff is **not** a crate dep (`rg -c git-cliff Cargo.lock` → no match).
- `changelog` mode: `git-cliff --config cliff.toml -o CHANGELOG.md` — writes a file,
  prints only a status line. **It does not print the changelog to stdout.**
  stdout form that works: `git-cliff --config cliff.toml --unreleased -o -`
  (INFO/WARN go to stderr; `-o /dev/stdout` is broken per archived research).
- `minor|patch` mode: dirty-tree guard → `git-cliff --bumped-version --bump <kind>`
  (stdout = version) → compare with newest tag `git tag --sort=-v:refname -l 'v[0-9]*'`
  → rewrite `[package] version` in root `Cargo.toml` → regenerate `CHANGELOG.md` with
  `--tag <ver>` → `git add Cargo.toml CHANGELOG.md` → commit
  `chore(release): prepare for <ver>` → annotated tag `<ver>`.
- `cliff.toml` (repo root, 105 lines): body groups `^feat` → 🚀 Features, `^fix` →
  🐛 Bug Fixes, doc/perf/refactor/style/test/chore|ci groups; `skip = true` for
  `chore(release): prepare for …`, `chore(deps.*)`, `chore(pr)`, `chore(pull)`;
  catch-all `.*` → 💼 Other. `[git]`: `conventional_commits = true`,
  `filter_unconventional = true`, `tag_pattern = "v[0-9].*"`, `sort_commits = "oldest"`,
  `topo_order_commits = true`. `[bump]`: `features_always_bump_minor = false`,
  `breaking_always_bump_major = false`, `initial_tag = "v0.1.0"`.
  Header marker: `<!-- git-cliff: end of header -->`.
- Live probe: `--bumped-version` → `v0.1.0` (WARN "No releases found, using initial tag
  'v0.1.0'"). `--unreleased -o -` prints the unreleased changelog to stdout.
- `CHANGELOG.md` exists (45 lines), only a `## [unreleased]` section — no release
  headers, because **there are no git tags yet** (`git tag | wc -l` → 0).

## 3. Version source

- Root `Cargo.toml:2-3`: `name = "virshot"`, `version = "0.1.0"`. All workspace
  member crates are `0.1.0` (`overlay`, `toast`, `gamepad`, `icons`, `video-capture`);
  `hidpad-poc` (excluded) is `0.1.0` too.
- Members: `[".", "crates/overlay", "crates/toast", "crates/gamepad", "crates/icons",
  "crates/video-capture"]`, `exclude = ["hidpad-poc"]`.
- No version constant in code: `src/main.rs:132` uses clap `#[command(version)]`
  (reads `CARGO_PKG_VERSION`); `tests/e2e/cli_headless.rs:74` asserts stdout contains
  `env!("CARGO_PKG_VERSION")`.
- ZeroVer policy: feat → patch, breaking → minor, `major` forbidden.

## 4. Build configuration and targets

- `.cargo/config.toml` is **fully commented out** (6 lines): no MinGW linker pinning,
  no `rustflags`. So docs claiming linker/`--subsystem,windows` pinning
  (`CONTRIBUTING.md:24`, `AGENTS.md` build section, `docs/spec.md:767`) are **not in
  effect**; the gnu build still links via default detection. Windows console
  suppression comes from `src/main.rs:64 #![windows_subsystem = "windows"]`.
- `[profile.release]`: `opt-level = "z"`, `lto = "thin"`, `strip = true`.
- Feature trimming lives in manifests, no CLI flags needed:
  `eframe default-features=false + ["glow","default_fonts","x11","wayland"]`,
  `image default-features=false + png/jpeg/webp`, `tray-icon default-features=false
  + ["ksni"]`, `xcap features=["wgc"]`.
- Target-specific deps: `[target."cfg(windows)".dependencies]` — root `gamepad`
  (path dep, zero crates.io deps), `crates/video-capture` → `windows-capture = "2.0.1"`.
- Repo-wide grep for target triples: only `x86_64-pc-windows-gnu` appears
  (CONTRIBUTING, README, cargo.sh comment, crate docs). **No** `msvc`, `aarch64`,
  `musl` anywhere in tracked files — the MSVC targets in this task are new.
- Local toolchain: `rustc/cargo 1.99.1`, `rust-toolchain.toml` = `channel = "stable"`
  (no pin). Installed targets: `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-gnu`,
  `x86_64-pc-windows-msvc`; **`aarch64-pc-windows-msvc` is not installed**.
- Build probes (warm cache, this host):
  - `build --release --target x86_64-pc-windows-gnu` → OK, 27.3s, exe **8,366,592 B**
    (under spec §5 budget <10 MB).
  - `build --release` (Linux) → OK, 1m15s, binary **15,124,864 B** — exceeds the
    spec §5 "<10 МБ" budget (spec §5/§10 assume Windows-only delivery).
  - `check --target x86_64-pc-windows-msvc` and
    `check --workspace --all-targets --target x86_64-pc-windows-msvc` → OK.
  - `build --release --target x86_64-pc-windows-msvc` → **compiles**, **link fails**:
    `error: linker 'link.exe' not found … the msvc targets depend on the msvc linker`.
    ⇒ MSVC release artifacts must be linked on a Windows runner.
  - `test --no-run --target x86_64-pc-windows-gnu` → OK (root targets only).

## 5. Native packages and gate scripts

- Linux dev packages (`CONTRIBUTING.md:11-14`):
  `pkg-config libegl1-mesa-dev libgbm-dev libudev-dev libwayland-dev
  libpipewire-0.3-dev libclang-dev` (failure modes quoted there:
  `unable to find library -lgbm`, `The system library 'egl' … not found`).
  `AGENTS.md` omits `libgbm-dev`; CONTRIBUTING includes it — use CONTRIBUTING's list.
- Windows cross-build (gnu): `apt install mingw-w64`; no native dev libs needed
  (`xcap` on Windows is pure Rust). Local: mingw-w64 11.0.1, `x86_64-w64-mingw32-gcc` present.
- `scripts/check.sh` (14 lines): `./cargo.sh check`; then
  `SKIP_E2E=1` → `cargo test -- --skip 'cli_headless::'`, else `cargo test`;
  then `./cargo.sh clippy -- -D warnings`. **Scope: root package only**
  (no `--workspace`, no `--all-targets`).
- `scripts/check_full.sh` (9 lines): `check.sh` → `cargo fmt` (**rewrites files**) →
  `cargo fmt --check`. CI must use `--check` only, never the rewriting half.
- `cargo.sh` (repo root): exports `CARGO_HOME=./.cache/cargo`, execs cargo —
  sandbox-only; CI runners use plain `cargo`.
- Verified locally (warm): `test --workspace` → 652 passed / 2 ignored, 3.75s;
  `clippy --workspace --all-targets -- -D warnings` → OK, 2.1s;
  `test -p overlay -p toast` → OK.

## 6. Test layout and CI execution shape

- Targets: `tests/e2e/main.rs` → `tests/e2e/cli_headless.rs` (312 lines);
  `tests/unit/main.rs` (placeholder, 0 tests — root is bin-only, no `lib` target);
  `crates/icons/tests/error.rs`, `crates/icons/tests/icons.rs`; sibling
  `src/*.tests.rs` modules; `crates/video-capture/src/*.tests.rs`; inline
  `#[cfg(test)] mod tests` in overlay/toast/gamepad/icons.
- Per-target counts (warm): gamepad 13, icons 2+1+7, overlay 50, toast 43,
  video_capture 117, virshot unit 410 (+1 ignored), e2e 9 (+1 ignored), unit target 0.
- Crate tests need explicit `-p` (plain `cargo test` = root package only):
  `-p overlay -p toast -p icons -p gamepad -p video-capture`, or `--workspace`.
  gamepad/video-capture tests run on Linux (Windows deps are `cfg(windows)`; the
  tests themselves are std-only).
- e2e (`tests/e2e/cli_headless.rs`) is **headless by design**: it asserts the failure
  path — no display → exit 1, `ERROR`/`Capture failed` in log; clap usage errors → exit 2.
  The only display-requiring test is `#[ignore]`d:
  `cli_shot_full_screen_success_prints_saved_path_and_exits_zero`
  (run with `cargo test -- --ignored` on a desktop host).
  Scratch dirs `./.tmp/cli-e2e-<tag>` — `.tmp` must exist/writable on the runner.
- `SKIP_E2E` is read only in `scripts/check.sh:7`; nothing in Rust code reads it.

## 7. Docs / spec constraints touching delivery

- `docs/spec.md:771-779` §10 "Тестирование и поставка": MVP delivery is
  `virshot.exe` straight from the build (`target/x86_64-pc-windows-gnu/release/…`),
  README.md, **no archive**, no installer, no auto-update; Windows acceptance is manual.
- `docs/spec.md:551` §5: binary size budget `< 10 МБ` (release strip/lto/opt-level=z).
- `docs/spec.md:707`: acceptance evidence = compile/clippy/release build for
  `x86_64-pc-windows-gnu`.
- README.md:10-12 repeats Windows-only delivery; README.md:40 claims "~0.84 MB"
  (stale vs measured 8.37 MB).
- ⇒ This task adds distribution scope (archives, Linux, MSVC targets, GitHub releases)
  that is **not** in `docs/spec.md` — it comes from the user's request, so the task
  itself is the scope change; spec.md (§project) is left untouched unless the user asks.

## 8. Other CI-relevant facts

- `rusty-hook` dev-dep (`Cargo.toml:75`): its vendored `build.rs` early-exits when
  `ci_info::is_ci()` → hooks are **not installed on CI**, so the pre-commit chain does
  not run in Actions. Local pre-commit chain:
  `SKIP_E2E=1 ./scripts/check.sh && ./cargo.sh fmt && git add -u . && ./cargo.sh fmt --check`.
- No `clippy.toml`, no `rustfmt.toml`; lint flags live in scripts (`-D warnings`,
  `fmt --check`). `#![forbid(unsafe_code)]` in root/icons/toast/video-capture;
  `#![deny(unsafe_code)]` + scoped allow in overlay/gamepad.
- `.gitignore`: `.tmp`, `.cache`, `target/`.
- git-cliff binary lives in git-ignored `.cache/cargo/bin/` → CI needs it
  (install from crates.io or prebuilt) — tool choice needs user approval per AGENTS.md
  (it is already approved as a dev tool, pinned 2.14.2).
- Cold-cache timings unknown (all probes were warm cache).

## 9. Formatting facts for the artifact/release contract

- The example `sha512` values are **base64 of the raw 64-byte SHA-512 digest**:
  88 base64 chars (`printf 'test' | openssl dgst -sha512 -binary | openssl base64 -A`
  → 88 chars, matches the sample's length).
- `release.yaml` sample fields: `version`, `releaseDate` (ISO-8601 with ms and Z),
  `commit_sha` (7-char short sha), `files[] { url, type (target triple), sha512, size }`.
  In the sample `url` is a bare filename, not a full download URL.
- Artifact names agreed with user (short tokens):
  `virshot-v<version>-win-x64.zip`, `virshot-v<version>-win-arm64.zip`,
  `virshot-v<version>-linux-x64.tar.gz`. Archives contain the binary only
  (`virshot.exe` / `virshot`).

## 10. Unknown — deeper research, planned as stages (not resolved in step 6)

- **`aarch64-pc-windows-msvc`**: target not installed locally, never documented in the
  repo. Unknown whether GitHub `windows-latest` (VS Build Tools with ARM64 cross
  component) can link it, and whether `xcap`/`eframe`/`glutin`/`windows-capture`
  build for Windows ARM64. Needs a verification stage.
- **Windows runner test execution**: whether `cargo test --workspace` passes on
  `windows-latest` headless (xcap monitor enumeration, tray/hotkey behavior) is untested;
  AGENTS.md only documents `--no-run` for Windows.
- **MSVC subsystem/linker behavior** on Windows runner vs MinGW pinning (config is
  commented out today) — to verify during the release-build stage.
- **CI install of git-cliff**: `cargo install` (slow, ~minutes) vs prebuilt binary
  download — needs a decision.
- **Prerelease flow**: GitHub releases require a tag ref; how a rolling in-place
  prerelease is represented (moving tag vs fixed non-`v` tag + asset delete/upload)
  and what `version`/`commit_sha` mean there — undecided.
- **Linux binary size**: 15.1 MB exceeds spec §5's 10 MB budget; whether the budget
  applies to Linux is undecided.
- Whether `url` in `release.yaml` is a bare filename or a full download URL.
  (Resolved with user: full download URL.)

## 11. Stage 1 baseline probe (2026-11-09, subagent run)

The Linux-job command set is already green on this host (warm cache), so CI is
additive, not corrective:

- `./cargo.sh fmt --all -- --check` → 0, no drift.
- `./cargo.sh check --workspace --all-targets` → 0 (5.05s warm).
- `./cargo.sh test --workspace` → 0, **652 passed / 2 ignored / 0 failed**.
- `./cargo.sh clippy --workspace --all-targets -- -D warnings` → 0, zero diagnostics.
- `git ls-files | rg -c '\.github'` → 0 — no CI files tracked.
- Ignored tests (never run on a headless runner):
  `src/capture.tests.rs:762 run_capture_full_screen_writes_valid_file`,
  `tests/e2e/cli_headless.rs:287 cli_shot_full_screen_success_prints_saved_path_and_exits_zero`.
- e2e scratch requirement verified locally: `./.tmp` exists and is writable;
  `tests/e2e/cli_headless.rs:32` creates `./.tmp/cli-e2e-<tag>` and cleans up on drop.
  ⇒ CI must ensure a writable workspace-relative `.tmp` (checkout root has it, or the
  job creates it).
- Local host has `DISPLAY`/`WAYLAND_DISPLAY`; GitHub Linux runner does not — that is
  exactly why the success-path tests are `#[ignore]`d.
- Native dev packages on this host: libudev-dev, libwayland-dev, libpipewire-0.3-dev,
  libclang-dev, libegl1-mesa-dev, pkg-config (`pkg-config --exists udev`/`egl` ok).
  `libgbm-dev` presence was not separately probed — CI installs it per CONTRIBUTING.md.

## 12. Deep research: `aarch64-pc-windows-msvc` on GitHub runners (Stage 2, 2026-11-09)

- **Rust support:** `aarch64-pc-windows-msvc` is **Tier 1 with host tools**
  (documented on https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html;
  the per-target page 404s). rustup components
  (`rust-std`/`rustc`/`cargo`/`clippy`/`rustfmt`) are available —
  https://rust-lang.github.io/rustup-components-history/aarch64-pc-windows-msvc.html.
  ⇒ `rustup target add aarch64-pc-windows-msvc` works on a runner.
- **`windows-latest` = `windows-2025-vs2026` (x64 host)**
  (https://raw.githubusercontent.com/actions/runner-images/main/README.md). The image
  ships `Microsoft.VisualStudio.Component.VC.Tools.ARM64` **and** `VC.Tools.ARM64EC`
  plus ARM64 ATL/MFC/Runtimes
  (https://raw.githubusercontent.com/actions/runner-images/main/images/windows/Windows2025-VS2026-Readme.md);
  `windows-2022` ships `VC.Tools.ARM64` too
  (https://raw.githubusercontent.com/actions/runner-images/main/images/windows/Windows2022-Readme.md).
- **Pitfall:** the *pinned* toolset flavor `VC.14.44.17.14.ARM64` is **absent** on
  `windows-2025-vs2026` while the x86/x64 flavor is present — filed fix
  https://github.com/actions/runner-images/pull/14555. ⇒ do **not** pin
  `-vcvars_ver=14.44` / PlatformToolset `v143`; use the default toolset (VS 18.11) so
  rustc's `vswhere` discovery finds the ARM64 `link.exe`.
- Corroboration that x64→ARM64 MSVC cross works on GitHub's x64 Windows runner:
  https://github.com/devhusdk/rules_dx/issues/6481 (closed, Nov 2026, "MSVC x64-to-ARM64
  cross outputs are ARM64 machine code", host = `windows-latest`).
- **Recipe (candidate, medium confidence on rustc discovery internals):**
  `rustup target add aarch64-pc-windows-msvc` + `cargo build --release --target …`,
  no `vcvars` step; explicit fallback
  `vcvarsall.bat x64_arm64 -vcvars_ver=14.29` (14.29 ARM64 flavor IS installed) and
  `CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_LINKER=link.exe` if env isn't inherited.
- **Native ARM64 runners exist** (`windows-11-arm`, `windows-11-vs2026-arm`;
  https://github.com/actions/runner-images/issues/14592) — they can *execute* ARM64
  binaries/tests; real projects use them for ARM64 CI
  (https://github.com/lopatnov/conduit/issues/671).
- **Crate set compiles on ARM64:** vendored locked sources contain **no x86 intrinsics
  and no `asm!`**; `windows-targets` provides dedicated ARM64 import libs
  (`windows_aarch64_msvc`). `xcap` (WGC via `windows` crate), `windows-capture`, winit
  (`windows-sys`), glutin/glow/gluin-winit, `global-hotkey`, `tray-icon`, `rfd` are
  arch-neutral; their cargo-c `default-target = "x86_64-pc-windows-msvc"` metadata is
  tooling info, not a consumer restriction. `crates/gamepad/src/gameinput.rs` uses
  `extern "system"` vtable slots ⇒ ABI-neutral on ARM64. Upstream never *tests* ARM64
  Windows for xcap/windows-capture/winit — compile-only evidence.
- **Runtime risk:** https://github.com/rust-lang/rust/issues/165346 (open, 2026-11-04) —
  miscompile on `aarch64-pc-windows-msvc` with **rustc 1.99.1** (this repo's stable):
  `catch_unwind` + `Mutex` program exits `0xC0000409`. An ARM64 binary of this project
  needs a real execution check (native ARM64 runner or manual hardware), not just a link.
- **Linux cross-build to msvc ARM64 is out:** Rust docs say non-Windows → `*-windows-msvc`
  "may be possible but is **not supported**"; tooling (`xwin`,
  https://github.com/mstorsjo/msvc-wine) is SDK-heavy (~1 GB download per job) and
  compile-only; `cargo-zigbuild` supports `aarch64-pc-windows-gnullvm` but **not**
  `aarch64-pc-windows-msvc`; Debian/Ubuntu have no `aarch64-w64-windows-gnu` package.
- **Research tooling limitation:** repo-scoped GitHub search (HTTP 422) and some
  raw.githubusercontent paths (404) blocked upstream claims for winit ARM64; conclusions
  rest on vendored source analysis. `web_search` (ddgs) returned no results in that run;
  all evidence came from direct URL fetches.

## 13. git-cliff invocation forms (empirical, git-cliff 2.14.2)

Verified in scratch repos with the repo's own `cliff.toml` (`tag_pattern = "v[0-9].*"`,
`initial_tag = "v0.1.0"`); `-o -` (stdout) and `-o <file>` produce byte-identical output,
so the workflow writes `-o dist/notes.md` and passes `--notes-file` to `gh`:

- **Tag release** → `git-cliff --latest -o dist/notes.md`: emits only the newest tag's
  range (commits since the previous tag). With the **first** tag and no previous tag it
  emits the initial range (all commits up to that tag) — exit 0, no crash.
- **Nightly / rolling prerelease** → `git-cliff --unreleased -o dist/notes.md`: commits
  after the latest `v*` tag. The rolling `nightly` tag is not a release boundary
  (`tag_pattern`), so it never truncates the unreleased range. With **zero tags** in the
  repo (today's state) it exits 0 and lists everything unreleased — the nightly path is
  safe before the first release.
- **Filtering:** `conventional_commits = true` + `filter_unconventional = true` drop
  non-conventional commits (git-cliff WARNs but exits 0); `render_always = true` means a
  range with only unconventional commits still produces a header-only `notes.md`, so
  `gh release … --notes-file` always gets a file.
- **`--tag <ver>`** was not needed: `--latest` already pins the newest tag's range, and
  for `v*` tag pushes the pushed tag is the newest one (checkout uses `fetch-depth: 0`,
  so tags are present in the runner clone).
