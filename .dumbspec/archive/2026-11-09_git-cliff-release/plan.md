# 2026-11-09_git-cliff-release: implementation plan (release helper)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

Operational note: git commits in this sandbox must run with `PATH` including
`./.cache/cargo/bin` so the installed rusty-hook hooks execute (observed
`rusty-hook: not found` on checkout otherwise).

---

## Stage 0 — Research + spec + plan

- [x] Capture draft.md; create task branch `2026-11-09_git-cliff-release`
- [x] Refinement gate: user decisions (tag `v0.1.2`, bump = commit + tag, `major` forbidden, git-cliff via cargo install with pinned version)
- [x] Research: repo/release state + git-cliff 2.14.2 docs (bump flags, `[bump]`, changelog, install) → research.md
- [x] Spec v1 + review: user resolved open questions (root-only bump, pin 2.14.2, default changelog template)
- [x] plan.md written

**Criterion:** task files draft.md / research.md / spec.md / plan.md exist; scope decisions recorded in spec.md Changes.
**Commit:** `chore(dumbspec): add git-cliff release task (draft, research, spec v1, plan)`

## Stage 1 — git-cliff dev tool (install + pin record)

- [x] Install pinned dev tool: `./cargo.sh install git-cliff --version 2.14.2 --no-default-features` (binary lands in `./.cache/cargo/bin/git-cliff`)
- [x] Verify: `./.cache/cargo/bin/git-cliff --version` reports 2.14.2
- [x] Record in AGENTS.md Dependencies: git-cliff is a dev **tool** (not a crate dependency), pinned 2.14.2, install command, binary location

**Criterion:** git-cliff 2.14.2 binary runnable from repo cache; AGENTS.md documents the pinned dev tool.
**Commit:** `chore(dev): pin git-cliff 2.14.2 dev tool (install + AGENTS.md note)`

## Stage 2 — cliff.toml (ZeroVer bump config)

- [x] Scaffold default template at repo root (`git-cliff --init`, default template per user decision)
- [x] Add ZeroVer config: `[bump] features_always_bump_minor = false`, `breaking_always_bump_major = false`, `initial_tag = "v0.1.0"` (empirical fix: must match `tag_pattern`); `[git] tag_pattern = "v[0-9].*"`
- [x] Verify bump plumbing against the installed binary (throwaway fixture repo): explicit version via `--bumped-version --bump <minor|patch>` (stdout has `v` prefix; flags combine), changelog regen via `--tag "v<ver>" -o CHANGELOG.md` (never an existing tag; `-o /dev/stdout` broken, use `-o -`); caveat recorded: explicit types do NOT bump when nothing is unreleased → release.sh needs an unreleased-commits guard

**Criterion:** `git-cliff --bumped-version` on the tagless repo yields a ZeroVer-compatible next version; the exact explicit-type invocation is verified and recorded in research.md.
**Commit:** `feat(release): add cliff.toml with ZeroVer bump config`

## Stage 3 — scripts/release.sh

- [x] Red first: fixture verification script `.tmp/release-fixture.sh` (throwaway repo under `./.tmp/`): 8 cases — minor/patch bump chains, major refused, changelog-only, dirty guard, no-unreleased guard, usage errors; run → red (release.sh missing, 8 failures)
- [x] Implement `scripts/release.sh` — subcommands:
  - `changelog`: `git-cliff -o CHANGELOG.md` only, no commit;
  - `minor` / `patch`: compute next version via git-cliff (Stage-2 verified invocation) → sed root `Cargo.toml` `[package] version` → regenerate `CHANGELOG.md` via `git-cliff --tag v<ver> -o CHANGELOG.md` → commit `chore(release): prepare for v<ver>` → annotated tag `git tag -a v<ver> -m "Release v<ver>"`;
  - `major`: fail with ZeroVer message (exit non-zero);
  - guard: refuse bump on dirty working tree; refuse bump when nothing is unreleased
    (git-cliff returns the current version with exit 0 — detect via unreleased commits);
  - style: `#!/usr/bin/env bash`, `set -euo pipefail`, `BASH_SOURCE` root resolution, English comments, git-cliff resolved from `PATH` with fallback `./.cache/cargo/bin/git-cliff`
- [x] Green: fixture run — `minor` 0.1.0→0.2.0, `patch` 0.2.0→0.2.1 (with an added unreleased fix commit — bump REQUIRES unreleased work; fixture semantics fixed in `.tmp/release-fixture.sh`), `major` exits non-zero, `changelog` writes CHANGELOG.md without commit; 8 passed / 0 failed, deterministic across runs
- [x] Refactor pass (tighten, no new abstraction): comment tightening + dropped redundant `-C "$ROOT"` after `cd "$ROOT"`; fixture still 8/8

**Criterion:** fixture verification passes all four cases; `bash -n scripts/release.sh` clean.
**Commit:** `feat(release): add scripts/release.sh (changelog + ZeroVer bump/tag)`

## Stage 4 — repo integration

- [x] Run `./scripts/release.sh changelog` in the real repo → `CHANGELOG.md` generated
- [x] Sanity: CHANGELOG.md covers the full commit history (unreleased section — no tags yet; first real release is run later by the user, not by this task)
- [x] Commit CHANGELOG.md (pre-commit gate ran; PATH included `./.cache/cargo/bin`)

**Criterion:** `CHANGELOG.md` exists at repo root covering the commit history; `release.sh changelog` runs cleanly; no tags were created.
**Commit:** `docs(release): generate initial CHANGELOG.md`

## Stage 5 — final acceptance

- [x] Independent acceptance subagent re-runs: fixture verification + repo checks (cliff.toml valid, release.sh executable and correct, AGENTS.md dev-tool entry)
- [x] `./scripts/check.sh` still passes (no Rust code touched)

**Criterion:** acceptance report green on fixture + repo state.
**Commit:** N/A
