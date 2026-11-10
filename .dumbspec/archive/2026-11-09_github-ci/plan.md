# 2026-11-09_github-ci: implementation plan (GitHub CI — tests + release builds)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = red verification → green implementation → refactor + a commit
at the end. This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan

- [x] Capture the raw request into `draft.md`; branch `2026-11-09_github-ci`.
- [x] Refinement confirmed with the user (artifact tokens, binary-only archives,
      rolling `nightly` prerelease, triggers, Windows full test run, full `url` in
      `release.yaml`, git-cliff stays an external tool installed in CI).
- [x] Superficial research scan (no `.github/`, release tooling, targets, test layout,
      native packages, CI constraints) recorded in `research.md`.
- [x] `spec.md` v1 written with decisions and open questions.
- [x] `plan.md` written.

**Criterion:** `draft.md`, `research.md`, `spec.md` (v1 with `Created`/`Changes`) and
`plan.md` exist under `.dumbspec/current/2026-11-09_github-ci/`; scope agreed.
**Commit:** `docs(ci): add GitHub CI spec and plan`

## Stage 1 — CI workflow: тестирование (`ci.yml`)

- [x] Baseline probe of the intended Linux job commands on this host: all green
      (`fmt --all -- --check`, `check --workspace --all-targets`,
      `test --workspace` 652 passed / 2 ignored, `clippy --workspace --all-targets
      -- -D warnings`) — recorded in research.md §11. CI is additive, no corrective
      red baseline exists.
- [x] Add `.github/workflows/ci.yml`: triggers `push` on `master` + `pull_request`
      (+ `workflow_dispatch`); concurrency group per ref; `permissions: contents: read`.
- [x] Linux job (`ubuntu-latest`): install native dev packages
      (`pkg-config libegl1-mesa-dev libgbm-dev libudev-dev libwayland-dev
      libpipewire-0.3-dev libclang-dev`), then
      `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`,
      `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Windows job (`windows-latest`): `cargo test --workspace` (full run),
      `cargo clippy --workspace --all-targets -- -D warnings`; if the headless e2e
      path misbehaves, degrade to `cargo test --workspace --no-run` and record the
      decision in `research.md`.
- [x] Local dry-run of the Linux job steps on this host: all four commands green
      (652 passed / 2 ignored). Windows proxy: `check --workspace --all-targets
      --target x86_64-pc-windows-msvc` green; `test --no-run --workspace
      --target x86_64-pc-windows-gnu` links 10 test binaries. MSVC linking and real
      Windows execution can only happen on a Windows runner.
- [ ] Verify on GitHub (user triggers): the workflow also exposes `workflow_dispatch`,
      so the branch can be run manually without widening the `push` triggers; triggers
      stay `master` + `pull_request`. Inspect the Actions run output.
- [x] Refactor: workflow stays minimal — only `actions/checkout@v4`, no `./cargo.sh`,
      no rewriting `cargo fmt`.

**Criterion:** one Actions run on the branch passes: Linux job green (fmt/check/test/
clippy, workspace scope) and Windows job green (full `cargo test --workspace` or the
recorded degraded form); no unapproved action used.
**Commit:** `feat(ci): add GitHub Actions test workflow` (local acceptance A–D green;
GitHub run pending user trigger)

## Stage 2 — Release workflow: build targets (scope narrowed)

- [x] Deep-check `aarch64-pc-windows-msvc` feasibility — done via web deep-research
      (research.md §12): Tier 1 + rustup components available, crate set compiles on
      ARM64 (no intrinsics/`asm!`), x64→ARM64 cross on `windows-latest` corroborated,
      runtime execution needs a native `windows-11-arm` check (rust #165346 on 1.99.1).
- [x] Scope decision (user, 2026-11-09T20:05+03:00): `aarch64-pc-windows-msvc` is
      **out of scope** for this task — ARM build will be a separate task when needed;
      artifact/release.yaml scheme stays unchanged so `win-arm64` can be added later.
- [x] Add `.github/workflows/release.yml`: triggers `push: tags: v*`, `push: branches:
      master`, `workflow_dispatch`; `permissions: contents: write`; concurrency group
      per ref; build job only (packaging/publish lands in Stage 3).
- [x] Build job matrix: `x86_64-pc-windows-msvc` on `windows-latest` (MSVC toolchain,
      `rustup target add`), `x86_64-unknown-linux-gnu` on `ubuntu-latest` with native
      dev packages; `cargo build --release --target …`. No toolset pinning.
- [x] Upload the produced binary (`virshot.exe` / `virshot`) as a workflow artifact
      keyed by target triple (`binary-win-x64`, `binary-linux-x64`, retention 5 days).
- [x] Local pre-verification: Linux release build green here (15,121,280 B stripped ELF
      at `target/x86_64-unknown-linux-gnu/release/virshot`); MSVC `check --workspace
      --all-targets --target …` green on a fresh target dir (97 crates, 0 warnings);
      MSVC release build fails locally with `error: linker 'link.exe' not found` —
      exactly why the runner builds it.
- [ ] GitHub run verification (user triggers; `workflow_dispatch` on the branch) —
      two artifacts present in the run summary.

**Criterion:** a workflow run produces two release binaries for the in-scope targets
(`win-x64` MSVC + `linux-x64`); artifacts keyed by target triple. ARM64 is deferred by
the user decision, not a failure.
**Commit:** `feat(ci): add release build workflow for msvc x64 and linux x64`

## Stage 3 — Packaging, `release.yaml`, publishing

- [x] Red baseline: the script was absent and `release.yml` had no packaging path; after
      implementation the negative runs (`missing binary`, `nightly` without `--sha`,
      bad `--kind`) fail with exit 1 and clear messages before anything is written.
- [x] `scripts/package-release.sh`: flags `--binaries DIR --version VER --kind
      release|nightly --sha SHORTSHA --repo OWNER/REPO --tag TAG [--out DIR]`; outputs
      `dist/virshot-v<version>-win-x64.zip` and `-linux-x64.tar.gz` (`-win-arm64.zip`
      naming scheme reserved but deferred with the ARM64 target)
      (nightly names add `-nightly-<shortsha>`), binary-only archives.
- [x] Hash/metadata: `sha512` = base64 of the raw digest
      (`openssl dgst -sha512 -binary … | openssl base64 -A`), `size` in bytes,
      `url` = full download URL `https://github.com/<owner>/<repo>/releases/download/<tag>/<file>`
      (owner/repo from `github.repository`, not hardcoded).
- [x] Generate `release.yaml`: `version` (root `Cargo.toml`, read by the workflow),
      `releaseDate` (ISO-8601 ms + `Z`), `commit_sha` (short),
      `files[] {url, type, sha512, size}`; validated locally with PyYAML (parse +
      field/order check + independent sha512 recomputation from the archive file).
- [x] Publish step in `release.yml` (`package` job on `ubuntu-latest`, `needs: build`):
      tag release → `gh release create <tag> --target-commit … <assets> release.yaml`;
      nightly → create with `--prerelease` if missing, else purge existing assets
      (`gh release delete-asset`) + `gh release upload` + `gh release edit
      --target-commit` (rolling in-place). `--notes-file dist/notes.md` when present,
      static `--notes` otherwise until Stage 4 supplies the changelog.
- [x] Local verification: script run against the Linux release binary + the gnu exe as
      a stand-in Windows binary; names/sizes correct; recomputed sha512 of each archive
      equals the YAML value; rerun into the same dir stays single-entry; publish step
      traced against a fake `gh` (`gh` is not installed here — publishing is unverified
      locally by design).

**Criterion:** `scripts/package-release.sh` produces correctly named archives and a
parseable `release.yaml` whose `sha512` reproduces locally; the publish path creates
the tag release and rolls the `nightly` prerelease in place.
**Commit:** `feat(ci): package release artifacts with release.yaml and publish via gh`

## Stage 4 — Release changelog (git-cliff)

- [x] Install git-cliff in CI: `cargo install git-cliff --version 2.14.2
      --no-default-features` (step in the `package` job on `ubuntu-latest`; no repo dep
      change — nothing in `Cargo.toml`/`Cargo.lock`).
- [x] Exact range command chosen empirically (research.md §13): tag release =
      `git-cliff --latest -o dist/notes.md` (newest tag's range; first tag = initial
      range), prerelease = `git-cliff --unreleased -o dist/notes.md` (commits after the
      latest `v*` tag; safe with zero tags). `--tag <ver>` was not needed.
- [x] Wire notes: `git-cliff … -o dist/notes.md` → `gh release create/edit
      --notes-file dist/notes.md` (the publish step already prefers `notes.md` when it
      exists; verified it survives packaging and never gets overwritten).
- [x] First release (no tags yet) and nightly (unreleased only) handled — exit 0 in both
      scratch scenarios; filtering (`filter_unconventional` + `render_always`) always
      yields a file, so the publish step never gets an empty notes path.
- [ ] Verify in a GitHub run (user triggers): tag release body contains grouped entries
      (🚀/🐛/…), nightly body contains the unreleased list.

**Criterion:** both release kinds carry a changelog body generated by git-cliff 2.14.2
in CI; no changelog logic duplicated in repo code.
**Commit:** `feat(ci): generate release changelog from git-cliff`

## Stage 5 — Docs + final acceptance

- [x] `CONTRIBUTING.md` CI section rewritten: what each workflow runs, native packages
      list (incl. `libgbm-dev` and its `rust-lld: error: unable to find library -lgbm`
      failure mode), git-cliff install in CI, release triggers, artifact naming table,
      `release.yaml` sample; CI is documented as plain `cargo`.
- [x] `README.md` delivery section updated (Russian): GitHub Releases with
      `virshot-v<version>-win-x64.zip` / `-linux-x64.tar.gz` + `release.yaml`
      (full URL, `sha512`, size) and the rolling `nightly` prerelease; the local
      gnu exe stays documented as the dev path.
- [x] `AGENTS.md` "Build & environment" (CI gates + Release delivery) and Dependencies
      "Release tooling" updated: CI uses plain `cargo` (the sandbox wrapper is
      local-only), git-cliff pinned 2.14.2 stays an external tool (nothing in
      `Cargo.toml`/`Cargo.lock`), `scripts/release.sh` never pushes/publishes, ARM64
      explicitly deferred out of scope.
- [x] Final gate locally: `./scripts/check.sh` green (docs subagent) and
      `./scripts/check_full.sh` green at the stage boundary (exit 0, no fmt drift).
- [ ] Full acceptance run on GitHub (user triggers): CI green on the branch via
      `workflow_dispatch`, one tag release and one nightly prerelease produced with
      matching `release.yaml` and changelog body.

**Criterion:** docs describe CI + release contract; local gates green; GitHub runs show
CI green and both release kinds published with correct artifacts/`release.yaml`/changelog.
**Commit:** `docs(ci): document CI and release publishing`

## Deferred (user-side)
- GitHub acceptance runs (the user triggers them; this session never pushes):
  `ci.yml` via `workflow_dispatch` on the branch, `release.yml` dispatch → rolling
  `nightly` prerelease, and a `v*` tag push → tag release; verify the published
  artifacts, `release.yaml` (full URL + base64 `sha512` + size) and changelog body.
  Real `cargo install git-cliff` timing is unknown until the first run.
- `aarch64-pc-windows-msvc` (`win-arm64`) build + ARM64 execution verification —
  separate future task (research.md §12 feasibility evidence); naming scheme and
  `release.yaml` already accommodate a third target.
- `cargo-audit` in CI — not part of this task, stays in `.dumbspec/backlog.md`.

## Execution notes

- GitHub verification: the **user** triggers Actions runs (confirmed 2026-11-09); this
  session does not push to `origin`. Workflows expose `workflow_dispatch` so runs can be
  launched on this branch without widening `push` triggers.
- Stage 2 ARM64 decision is resolved: target deferred out of scope by the user; if ARM64
  is needed later, open a separate task (research.md §12 has the feasibility evidence).
- Nothing here adds crates.io dependencies; git-cliff stays an external tool (approved
  pinned version 2.14.2).
