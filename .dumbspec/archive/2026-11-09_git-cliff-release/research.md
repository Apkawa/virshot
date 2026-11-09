# Research — git-cliff release helper

Sources: repo scan + cloned git-cliff docs (`.tmp/git-cliff/website/docs/`) + git-cliff's own `release.sh`. Research report from subagent b4cb681a, verified facts below.

## A. Repo/release state

1. **Root Cargo.toml**: `name = "virshot"`, `version = "0.1.0"`, `edition = "2024"`. Workspace:
   `members = [".", "crates/overlay", "crates/toast", "crates/gamepad", "crates/icons", "crates/video-capture"]`, `exclude = ["hidpad-poc"]` — SIX members; `crates/icons` and `crates/video-capture` are newer than the AGENTS.md snapshot. Every member crate also declares `version = "0.1.0"` (overlay, toast, gamepad, icons, video-capture). Root package is the shipped binary; crates are path deps.
2. **CHANGELOG.md**: does not exist at repo root. **cliff.toml**: does not exist anywhere in the repo (only inside `.tmp/git-cliff/` clone). `cargo.sh` lives at repo ROOT, not under `scripts/`.
3. **Git tags**: none (`git tag -l` empty). First release ever. Branch `2026-11-09_git-cliff-release`, 28 commits total.
4. **Commit history**: all 28 commits match Conventional Commits `type(scope): subject` (verified 28/28, zero non-matching). No `!:` breaking markers in history.
5. **scripts/** conventions: `check.sh`, `check_full.sh`, `cratedoc.sh`, `gen-icons.sh`, `prof.sh`, `ui-verify.sh`. Shell style to mirror: `#!/usr/bin/env bash` (cargo.sh) or `#!/bin/bash` (scripts/*), `set -euo pipefail`, repo-root resolution `here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"`, cargo ONLY via `./cargo.sh`, English comments.
6. **git-cliff installed**: NO (`git-cliff --version` exit 127). `.cache/cargo/bin/` has only `rusty-hook`. Install path in sandbox: `./cargo.sh install git-cliff` (CARGO_HOME=./.cache/cargo; global ~/.cargo read-only). Toolchain: rustc 1.99.1 ≥ git-cliff MSRV 1.88.0.

## B. git-cliff 2.14.2 facts (cloned docs)

7. **Bump flags** (usage/bump-version.md, args.md):
   - `git cliff --bump [auto|major|minor|patch]` — NOT `--bump-version`.
   - `git cliff --bumped-version` — prints the next version to stdout.
   - Rules: `fix:`→PATCH, `feat:`→MINOR, `scope!`→MAJOR; bump checked against `[git] tag_pattern`.
   - Config section is **`[bump]`** (configuration/bump.md): `features_always_bump_minor`, `breaking_always_bump_major`, `initial_tag` ("version set to this value if no tags are found"), `custom_major_increment_regex`, `custom_minor_increment_regex`, `no_increment_regex`, `bump_type`.
   - **v1-era keys do NOT exist in 2.14.2**: `zerover`, `version_bump_type`, `--bump-version`, `[version]`, `tag_prefix` — zero hits in docs/clone. ZeroVer equivalents: `features_always_bump_minor=false` → feat bumps PATCH when major is 0; `breaking_always_bump_major=false` → breaking bumps MINOR when major is 0.
   - **git-cliff does NOT write Cargo.toml and does NOT create tags.** `--bump` feeds the bumped version into the changelog entry; `--bumped-version` prints it. Tag creation is user-side (docs adding-tag-messages.md: `git tag v1.0.0 -m "..."`).
8. **Changelog generation** (index.md quickstart): `git-cliff --init` creates `cliff.toml`; `git cliff -o CHANGELOG.md` (default output `CHANGELOG.md`); incremental: `git cliff --unreleased --tag 1.0.0 --prepend CHANGELOG.md`. Config discovery: cliff.toml → .cliff.toml → .config/cliff.toml → parents → global. Default template lives in clone `.tmp/git-cliff/config/cliff.toml` (96 lines): header/body Tera renders `## [{{ version | trim_start_matches(pat="v") }}] - {{ timestamp }}` else `## [unreleased]`; commit_parsers group 🚀 Features / 🐛 Bug Fixes etc., SKIP `^chore\(release\): prepare for` (git-cliff's release-prep commit), `^chore(deps.*)`, `^chore(pr)`, `^chore(pull)`; default template has NO `tag_pattern` line. Example configs in `.tmp/git-cliff/examples/` (keepachangelog, github, detailed, minimal, scoped...).
9. **Installation**: `cargo install git-cliff` (crates.io), MSRV 1.88.0, default features update-informer/github/gitlab/gitea/bitbucket. Cloned repo corresponds to **git-cliff 2.14.2** (`git-cliff/Cargo.toml` `version = "2.14.2" # managed by release.sh`; CHANGELOG top entry `## [2.14.2] - 2026-09-18`). crates.io may be newer — network check not performed.
10. **Bump↔changelog interaction**: `git-cliff --bump --unreleased` builds changelog for the future version; `-u/--unreleased`, `-l/--latest`, `--current`, `-t/--tag <TAG>` (sets tag name for the latest version — used before the tag exists), `--with-tag-message [MSG]`. **Zero tags case**: `[bump] initial_tag` governs the starting version.
11. **git-cliff's own release.sh** (`.tmp/git-cliff/release.sh`, canonical pattern): sed version into Cargo.toml (`# managed by release.sh` marker) → `git cliff --config cliff.toml --tag "$1" > CHANGELOG.md` (full regeneration) → `git add -A && git commit -m "chore(release): prepare for $1"` → `git tag -s -a "$1" -m "Release $1" ...` → push manually. Monorepos doc: per-directory changelog via `cd crates/x && git cliff` or `--include-path/--exclude-path`; no multi-manifest bump feature.

## Constraints found

- git-cliff absent; install via `./cargo.sh install git-cliff` — new tool, requires user approval per AGENTS.md dependency policy (user already approved cargo install + pinning).
- git-cliff 2.14.2 API: `--bump`/`--bumped-version`, `[bump]` config, `[git] tag_pattern`; NO `zerover`/`version_bump_type`/`--bump-version`/`tag_prefix`.
- Version writing into manifests and tag creation must be done by `release.sh` itself.
- No tags exist → `[bump] initial_tag` must be set (repo version is 0.1.0); `tag_pattern = "v[0-9].*"` needed for `v*` tags.
- Any commit from release.sh triggers rusty-hook pre-commit gate (`SKIP_E2E=1 ./scripts/check.sh && cargo fmt ...`); version-only commits still pay the gate cost.
- Shell conventions: root-relative `BASH_SOURCE` resolution, `set -euo pipefail`, cargo via `./cargo.sh` only, English comments.

## Decided by user (2026-11-09)

- Tag: `v<version>` (e.g. `v0.1.2`); release bump = commit + tag on that commit; `major` subcommand forbidden; git-cliff via `cargo install`, version pinned in project.

## Still open → resolved at spec review (2026-11-09)

- Bump scope: **root `Cargo.toml` only** (user decision 2026-11-09; crates stay 0.1.0; git-cliff has no multi-manifest bump anyway).
- Pin git-cliff version: **2.14.2** (user decision).
- Tag style: annotated `git tag -a`, no signing (git-cliff's release.sh uses `-s -a`; no GPG key in repo).
- CHANGELOG.md committed inside the release commit (git-cliff release.sh does `git add -A`).
- rusty-hook pre-commit gate on release commits: accepted (rare event).
- Changelog template: git-cliff **default** (`--init`) (user decision).

## Empirical verification (installed git-cliff 2.14.2 binary, Stage 1)

- Installed via `./cargo.sh install git-cliff --version 2.14.2 --no-default-features`
  (approved feature choice: drops update-informer/forge notifier; core CLI intact).
  Binary: `./.cache/cargo/bin/git-cliff`; `--version` → `git-cliff 2.14.2`.
- `--bumped-version` is a value-less FLAG: does NOT accept an explicit bump-type
  argument; for a `feat:` commit in a tagless throwaway repo it printed `0.1.0`.
- `--bump [<BUMP>]` accepts an optional explicit type (args.rs: `num_args=0..=1`,
  `default_missing_value="auto"`, parser accepts exactly `auto|major|minor|patch`);
  empirically `--bump auto|major|patch` and bare `--bump` accepted;
  `--bump bogus` rejected: `invalid value 'bogus' for '--bump [<BUMP>]'`.
- Consequence for release.sh: explicit minor/patch must go through `--bump <type>`;
  version retrieval via `--context` JSON (or computed tag string) — exact
  invocation verified in Stage 2.

## Deviation found while creating cliff.toml (Stage 2, empirical)

- `initial_tag = "0.1.0"` FAILS in 2.14.2: with zero tags the next version is
  the initial tag verbatim and it must match `[git] tag_pattern` — error
  `Changelog error: Next version (0.1.0) does not match the tag pattern: v[0-9].*`
  (git-cliff-core release.rs: `None => version: config.get_initial_tag()`;
  lib.rs rejects mismatched next version).
- Fix keeping intent: `initial_tag = "v0.1.0"` (matches tag_pattern, v-prefix
  convention). Validation now passes: `--bumped-version` prints `v0.1.0` (exit 0,
  WARN "No releases found, using initial tag 'v0.1.0'").
- First-release semantics consequence: until a tag exists, `--bump`/`--bumped-version`
  return the initial tag itself — so the FIRST release tags/publishes the current
  version (v0.1.0), regardless of requested minor/patch; commit-driven ZeroVer
  bumps apply only after a tag exists. Throwaway repo verified: after tag v0.1.0 —
  fix → v0.1.1, feat → v0.1.1 (patch, major==0), feat! → v0.2.0 (minor), v-prefix kept.

## Verified release.sh invocations (Stage 2, throwaway repo, git-cliff 2.14.2)

- Explicit version: `git-cliff --config cliff.toml --bumped-version --bump <minor|patch>`
  — the two flags combine; explicit minor overrides ZeroVer auto (auto gave v0.1.1,
  explicit minor → `v0.2.0`). stdout INCLUDES the `v` prefix, newline-terminated;
  INFO/WARN go to stderr. Accepted `--bump` values: auto|major|minor|patch only;
  anything else → exit 2, empty stdout. `--bump major` under ZeroVer prints v0.1.1
  (keeps major=0) — release.sh still forbids major itself.
- Changelog regen: `git-cliff --config cliff.toml --tag "v<ver>" -o CHANGELOG.md`
  renders unreleased commits under `## [<ver>]` plus older releases. NEVER pass an
  existing tag (duplicates the section). `-o /dev/stdout` fails (IoError, exit 1) —
  use `-o CHANGELOG.md` or `-o -`.
- No-op guard needed: after everything is tagged, bare/explicit `--bump*` all print
  the CURRENT version (exit 0) — explicit types do not bump with nothing unreleased.
  release.sh must check unreleased commits (e.g. `git log "<last tag>..HEAD"` count)
  and fail with a clear message.
- jq exists (/usr/bin/jq); `--unreleased --bump <type> --context` JSON has top-level
  `"version":"v0.2.0"` — usable fallback, jq-free extraction verified
  (`grep -o '"version": *"v[^"]*"' | head -1`).
