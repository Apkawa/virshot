# 2026-11-16_ci-optimization: implementation plan (CI optimization)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red check → implementation → refactor) + a commit at the end.
For CI-YAML work the "test" is a static verification of `.github/workflows/ci.yml`
(YAML parse + structural assertions) run before the change (fails) and after it (passes);
plus, where possible, a local smoke run of the exact shell snippet in `./.tmp/`.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan
- [x] Create task dir, draft.md, initial spec skeleton
- [x] Refinement gate confirmed by user (proceed as-is)
- [x] Research subagent: local workflow scan, git-cliff binary releases (assets, `.sha512` format, gnu vs musl), Swatinem/rust-cache (v2.9.3 SHA, inputs, defaults)
- [x] research.md written from findings
- [x] spec.md refined to v1 (gnu chosen, `.sha512` via `sha512sum -c`, rust-cache SHA-pinned v2.9.3, save-if master-only)

**Criterion:** research.md, spec.md (v1, Created + Changes), plan.md exist; spec has no open questions.
**Commit:** `docs(dumbspec): ci-optimization task — draft, research, spec v1, plan`

## Stage 1 — git-cliff binary download in `release` job
- [x] Red check: assert ci.yml still contains `cargo install git-cliff` (baseline)
- [x] Replace step `Install git-cliff` with `Download git-cliff`: wget tar.gz + `.sha512` from GitHub releases (pin 2.14.2, gnu), `sha512sum -c` verification, `tar -xzf` extract
- [x] Update `Generate release notes` to invoke `./git-cliff-2.14.2/git-cliff` (relative path, no PATH install)
- [x] Local smoke test of the exact snippet in `./.tmp/` (download → verify → extract → `--version`)
- [x] Refactor: keep comments minimal, no extra abstraction

**Criterion:** `.github/workflows/ci.yml` has no `cargo install git-cliff`; contains wget of `git-cliff-2.14.2-x86_64-unknown-linux-gnu.tar.gz` + `.sha512`, `sha512sum -c`, and calls the extracted binary; local smoke run prints `git-cliff 2.14.2`.
**Commit:** `ci: use git-cliff binary release with sha512 verification instead of cargo install`

## Stage 2 — Swatinem/rust-cache in cargo jobs
- [x] Red check: assert ci.yml has no rust-cache step (baseline)
- [x] Add SHA-pinned rust-cache (`@d1527c8f67ef9af357290475d4af4b0fab8f12e0 # v2.9.3`) to `linux` job (after apt install, before cargo steps)
- [x] Add the same step to `windows` job (after checkout, before cargo steps)
- [x] Add the same step to `build` job (after `rustup target add`, before `cargo build --release`)
- [x] Inputs on all three: `cache-on-failure: true`, `save-if: ${{ github.ref == 'refs/heads/master' }}`
- [x] Green check: YAML parses; rust-cache present in exactly the 3 cargo jobs, pinned by SHA

**Criterion:** rust-cache step exists in `linux`, `windows`, `build` with the pinned SHA and both inputs; `ci.yml` parses as YAML.
**Commit:** `ci: add Swatinem/rust-cache for cargo builds`

## Stage 3 — Acceptance
- [x] Acceptance subagent: independently verify Stages 1–2 criteria against ci.yml (structure + snippet correctness), YAML parse, and re-run the git-cliff download/verify smoke in `./.tmp/`
- [x] Fix anything the acceptance flags (if none — close out)

**Criterion:** acceptance report confirms: no `cargo install git-cliff`, wget+sha512sum verification, extracted-binary invocation, rust-cache pinned in all 3 cargo jobs with `cache-on-failure`/`save-if`; smoke run outputs `git-cliff 2.14.2`.
**Commit:** N/A
