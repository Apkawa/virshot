# 2026-11-11_auto-update: implementation plan (Automatic update)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan

- [x] Raw request captured into `draft.md` (verbatim, Russian)
- [x] Initial spec skeleton written from the draft (no research)
- [x] Refinement gate confirmed by user ("proceed to research")
- [x] Superficial repo scan delegated to a research subagent; findings appended to `research.md`
- [x] Spec refined to v1 (Created + Changes), blocking questions asked and answered
- [x] Spec updated to v2 with the approved decisions (deps bundle, both platforms, API release body, immediate restart, defaults + timer, General-tab row + toast)
- [x] This plan written

**Criterion:** `draft.md`, `research.md`, `spec.md` (v2) and `plan.md` exist under `.dumbspec/current/2026-11-11_auto-update/`; the approved scope and dependency set are recorded in `spec.md` with a dated Change entry.
**Commit:** `bedf085 docs(spec): add auto-update task spec, research, and plan`

---

## Stage 1 — Dependency pinning and size budget

- [x] Verify locked APIs/features before adding: `./scripts/cratedoc.sh` for `ureq`, `sha2`, `zip`, `self_replace`; pick minimal feature sets (`ureq` = rustls TLS + redirect-following, no gzip; `zip` = deflate only; `self_replace` default)
- [x] Check whether `serde_json` is already in `Cargo.lock` transitively; if not, ask the user for approval before declaring it
- [x] Add `self_replace`, `ureq`, `sha2`, `zip`, declared `flate2` (+ `serde_json` if approved) to root `Cargo.toml`, feature-trimmed
- [x] `./scripts/check.sh` green on Linux; `./cargo.sh check --target x86_64-pc-windows-gnu` green
- [x] Measure `./cargo.sh build --release --target x86_64-pc-windows-gnu` binary size against the < 10 MB budget; append the numbers and the `Cargo.lock` churn to `research.md`

**Criterion:** both gates green with the new deps present; release-build size recorded and within budget (or a documented decision to trim further); lock churn documented in `research.md`.
**Commit:** `8838259 feat(deps): add self-update dependency set (ureq, sha2, zip, flate2, serde_json, self-replace)`

---

## Stage 2 — Settings model (General: check interval, channel, auto-install)

- [x] Red: tests in `src/config.tests.rs` — `UpdateCheck` / `UpdateChannel` serialize as lowercase strings (pattern of `Format`), `#[serde(default)]` fills defaults when keys are absent, unknown values rejected by the existing lenient parse path, defaults `1d` / `stable` / `auto_install = false` via `general_with_home()`
- [x] Green: add `update_check`, `update_channel`, `auto_install` to `General` (`Eq` + serde), enum types with `Default`, defaults wired
- [x] Add UI strings to `locales/app.yml` for all six locales; update the pinned key-count assertions in `src/i18n.tests.rs`
- [x] Refactor; `./scripts/check.sh` green

**Criterion:** config round-trip + defaults tests green, existing settings tests untouched and green, catalog test passes with the new key count on all six locales.
**Commit:** `33bd4a8 feat(config): add auto-update settings to General`

---

## Stage 3 — `src/update.rs`: metadata fetch and update decision

- [x] Red: `src/update.tests.rs` — `release.yaml` parsing (version, quoted releaseDate, short commit_sha, `files[]` url/type/sha512/size), platform file selection (`x86_64-pc-windows-msvc` on Windows incl. `-gnu` dev builds, `x86_64-unknown-linux-gnu` on Linux, unknown `type` → typed error), decision rule (version **or** sha differ ⇒ update), `-dev` build skips the check, `Never` ⇒ no check
- [x] Red: interval scheduling tests with injected `now` — deadline for `Hour`/`Day`/`Week`, no deadline for `Never`
- [x] Green: `ReleaseManifest` model parsed with **`serde_saphyr::from_str`** (amendment 2026-11-11, user-approved: the shipped `release.yaml` asset is YAML colon syntax — strict `toml` rejects it; serde-saphyr 1.3.0 is already in `Cargo.lock` transitively via rust-i18n, so declaring it adds ZERO new packages); pure functions `select_file`, `decide_update`, `next_check_deadline`; typed `thiserror` enum for fetch/parse failures
- [x] Background fetch: plain `std::thread::spawn` + `mpsc`, fetch function injectable so no unit test touches the network; stable URL `releases/latest/download/release.yaml`, nightly URL `releases/download/nightly/release.yaml`; timeouts, errors logged and delivered as a value
- [x] Refactor; `./scripts/check.sh` green

**Criterion:** `src/update.tests.rs` green (parse, selection, decision, schedule) with no network in tests; the fetch thread compiles on Linux and Windows-gnu targets.
**Commit:** `a1d3982 feat(update): add release metadata check, decision, and scheduling`

---

## Stage 4 — `src/update_notes.rs`: release body for the update dialog

- [x] Red: `src/update_notes.tests.rs` — GitHub release JSON parsing (`body` for stable `/releases/latest`, nightly `/releases/tags/nightly`), fallback decision when the fetch fails (summary from `release.yaml` + release page URL), no network in tests (fixture JSON)
- [x] Green: blocking `ureq` GET with UA header, typed error `NotesError::{Fetch, HttpStatus, Parse}`; shape deviation (KISS): instead of `NotesSource::{Body, Summary}` the module returns `ReleaseNotes { body, html_url }` with an empty `body` meaning "no notes" and `release_page_url(channel)` supplying the fallback link — the GUI composes the fallback in Stage 6 from data it already has (`UpdateInfo`)
- [x] Refactor; `./scripts/check.sh` green (approved scope deviation: 3 `vec![]` asserts in `src/gamepad.tests.rs` annotated `Vec::<u8>::new()` — declaring `serde_json` links it into the bin and adds a `u8: PartialEq<serde_json::Value>` candidate, so element-type inference no longer resolves; semantics unchanged)

**Criterion:** notes parsing and fallback tests green; fetch happens only when the dialog is opened (no startup API request), verified by wiring in Stage 6.
**Commit:** `61c49ea feat(update): fetch release notes for the update dialog`

---

## Stage 5 — `src/update_install.rs`: download, verify, unpack, self-replace, restart, cleanup

- [x] Red: `src/update_install.tests.rs` — base64 decoder for the 88-char sha512 string (valid/invalid length, padding), digest comparison helper (sha2 against a known blob), tar reader over synthetic tar bytes (single regular-file entry, size mismatch → error), zip extraction of a synthetic one-entry archive into a scratch dir under `./.tmp/`, leftover selection (which leftover names are deleted, missing files tolerated)
- [x] Green: temp-dir layout (`std::env::temp_dir()` + dedicated subdir `virshot-update-<sha>`, remote sha sanitized to `[0-9a-f]` before it becomes a path component), download to temp file (`size` enforced as a mid-stream bound), unpack dispatch by archive extension (`.zip` → zip reader, `.tar.gz` → `flate2` + hand-written ustar reader — both paths compile on both targets), `replace` seam (production passes `self_replace::self_replace`; tests inject a fake — the dep itself is declared at HEAD `8838259`, Cargo.toml:87), temp dir removed best-effort on success AND failure
- [x] Verification amendment: sha512 is verified by a post-write re-read (`verify_archive`) rather than hashing the download stream — strictly stronger, catches truncated writes; documented in the module; plan wording kept as "size + sha512 verification before unpack"
- [ ] Restart sequence (moves to Stage 7 wiring): release `InstanceGuard` (removes `<config>.lock`), `Command::new(std::env::current_exe())` detached spawn, exit 0; failure to spawn → log + keep the old process alive (Open question 3 fallback)
- [x] `cleanup_leftovers()` implemented for startup (called from Stage 7): real self-replace 1.5.1 leftover names (`virshot*.__relocated__.exe`, `virshot*.__temp__.exe`, unix `.<stem>.__temp__*`, `virshot*.__selfdelete__.exe` — self-replace never emits `*.old`, plan wording corrected) + stale `virshot-update-*` dirs in temp dir, `NotFound` ignored, errors logged only
- [x] Refactor; `./scripts/check.sh` green; `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green; `./cargo.sh clippy --workspace --all-targets -- -D warnings` green (CI variant; one `io_other_error` lint in the test file fixed — `std::io::Error::other`)

**Criterion:** pure parts unit-tested deterministically (decoder, digest, tar, zip, leftover selection); Linux unpack verified against the REAL nightly asset (5,617,967 B, digest byte-identical to `release.yaml`, extracted `virshot` 15,099,256 B — `#[ignore]` test green); Windows path compiles and is runtime-accepted later on a Windows host.
**Commit:** `84fd712 feat(update): download, verify, unpack, and self-replace updates`

---

## Stage 6 — UI: General controls, "Update available" row, dialog, toast

- [x] Red: pure decision tests in `src/gui.tests.rs` — visibility of the "Update available" row (found / none / dev build / `Never`), dialog button outcomes (Install → run install, Cancel → dismiss), enabled state of the row while an install is in flight
- [x] Green: General tab controls — `ComboBox` for `update_check` (никогда/1h/1d/1w) and `update_channel` (stable/nightly), `ui.checkbox` for `auto_install` (pattern of the Overlay tab), localized labels via `rust-i18n`
- [x] Green: "Доступно обновление" row in the General tab + toast chip on discovery (existing toast chip plumbing); update dialog as its own egui viewport (like About), body rendered with `egui_commonmark`, buttons «Обновление» / «Отмена», notes fetched lazily on open
- [x] Drain the check/install channels in `App::logic`; settings persist only through `apply()`; draft cancel resets to applied
- [x] Refactor; `./scripts/check.sh` green (527 unit / 9 e2e; CI variant `clippy --workspace --all-targets` green)

**Criterion:** decision tests green; controls render with localized labels in all locales; dialog opens with notes or the summary fallback; install button triggers Stage 5 flow.
**Commit:** `49775fc feat(gui): add auto-update controls, discovery row, and update dialog`

---

## Stage 7 — Startup wiring, timer, resident mode

- [x] Red: pure startup-decision test — check runs at startup only when interval ≠ `Never` and the build is not `-dev`; leftover cleanup always runs (6 pure tests in `src/main.tests.rs`; red baseline = these symbols/tests do not exist at `49775fc`)
- [x] Green: `src/main.rs` — leftover cleanup step after logging init (before `InstanceGuard::acquire`), first background check spawn, next-check deadline honored in `App::logic` (500 ms tick), GUI-less `park_without_gui`: auto-install still runs, no toast/window when auto-install is off
- [x] Channel ownership amendment (found at Stage 6 acceptance): the check channel is created inside `gui::run` with the sender dropped, so `main.rs` creates it and passes the `Receiver` in (and owns the `Sender` for spawn + deadlines)
- [x] Restart wiring amendment: install success sets a shared `restart_requested` flag + closes the root viewport; after `gui::run` returns `main.rs` releases `InstanceGuard`, `Command::new(std::env::current_exe())`, exit 0; spawn failure → log + keep the old process alive (Open question 3 fallback)
- [x] Retry nuance: a NEW discovery resets `UpdateStatus::Failed` → `Idle` so the row regains the install button (in-process retry without restart)
- [x] Dead-code cleanup amendment: re-point the dialog notes fetch to `update_notes::fetch_release_notes` (seam becomes `Fn(&str) -> Result<ReleaseNotes, NotesError>`), then remove the module-wide `#![allow(dead_code)]` in `update.rs`, `update_notes.rs`, `update_install.rs` and the `#[allow(dead_code)]` on `UpdateCheck::interval()` — shipped code must surface real dead code
- [x] Verify CLI single-shot mode (`virshot shot`, `--version`) never triggers a network request (call sites outside `run_shot`/`--version`; empirical: 0 update/manifest/github log lines)
- [x] Refactor; `./scripts/check.sh` green (with `SKIP_E2E=1`) and `./cargo.sh test --test e2e` green on a display-capable host

**Criterion:** startup decision unit-tested; startup order documented; CLI/headless e2e unchanged and green.
**Commit:** N/A

---

## Stage 8 — Product spec amendment and final gates

- [x] `docs/spec.md`: new auto-update section (settings, sources, install flow, cleanup, privacy note); revise §5 network/privacy wording and §10 delivery wording ("без инсталлятора и автообновления" → shipped)
- [x] `./scripts/check_full.sh` green (incl. `cargo fmt --check`); `./cargo.sh test --workspace` green; `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green
- [ ] Push the branch / open a PR and confirm CI (Linux + Windows jobs) passes — blocked in this sandbox (`git push` is denied); CI runs on the PR the user opens
- [x] Record Windows runtime acceptance as deferred (same convention as the gamepad hardware run): real download → replace → relaunch on a Windows host

**Criterion:** product spec documents the feature; Linux gates + Windows compile gates green; Windows runtime acceptance and CI (PR) explicitly deferred with notes in `docs/spec.md`.
**Commit:** `docs(spec): document auto-update, revise size budget and delivery`

---

## Stage 9 — Update section header, forced-check button, dev-build checks (user amendment 2026-11-11T23:31)

- [x] Red: unit tests — a `-dev` build performs the comparison (update available when `commit_sha` differs, version suffix `-dev` never compared); startup gate arms the check for a dev build when the interval is scheduled; forced-check request is honored even when the interval is `Never`; one check in flight disables the button
- [x] Green `src/update.rs`: dev comparison by `commit_sha` only; drop the dev short-circuit in `run_check`/`spawn_check`
- [x] Green `src/main.rs`: startup gate (`should_check_at_startup`) and `park_without_gui` deadline no longer exclude dev builds
- [x] Green `src/gui.rs`: section header **«Обновление»** inside General, label **«Проверять обновления»** above the interval dropdown, button **«Проверить обновления»** below it (disabled while a check is in flight; forced spawn leaves the interval deadline untouched); the discovery row is now gated on discovery only, so a forced check with the interval set to `никогда` shows its result
- [x] `locales/app.yml`: +3 keys (`update.section_title`, `update.check_label`, `update.check_now_button`) × 6 locales; `i18n.tests.rs` pins 92 → 94 (`update.check_label` replaced the now-dead `general.update_check_label`, which was dropped with a `…drops_interval_label_key…` test mirroring the About precedent)
- [x] Re-point code comments that cite "spec §4.6" for auto-update to §4.7 (§4.6 is the About dialog) — every such reference in `src/` and `locales/` is gone; the planned `src/update_install.rs:7` site turned out to cite stage numbers, not §4.6
- [x] Acceptance fix (pass-1 drain/arm race): arm the deadline before draining outcomes and decide `check_in_flight` in one place (`check_in_flight_after`) — a startup outcome draining on the first logic pass used to leave the slot permanently occupied (no scheduled checks, button disabled for the session)
- [x] Acceptance fix (schedule follows Apply): `schedule_after_apply` re-arms/clears the deadline from the applied interval (`никогда` → scheduled arms without a restart; `1h` → `1d` re-arms; scheduled → `никогда` no longer fires one extra check); an in-flight check is never cancelled
- [x] Acceptance fix (press not swallowed): a «Проверить обновления» press recorded while the slot is busy stays pending and runs when the slot frees (still exactly one check in flight)
- [x] Refactor + gates: `./scripts/check.sh`, `./cargo.sh clippy --workspace --all-targets -- -D warnings`, `./cargo.sh check --target x86_64-pc-windows-gnu`, `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`; boundary `./scripts/check_full.sh` green — unit 549 passed / 2 ignored, e2e 9 passed / 1 ignored

**Criterion:** dev build reports an update when the release sha differs; the button forces a check with the interval set to `никогда`; exactly one check in flight and the interval deadline is moved only by scheduled outcomes; the row/dialog/toast path is unchanged for discoveries.
**Commit:** `feat(update): add the update settings section, forced check, and dev-build checks`

---

## Stage 10 — Product spec follow-up for the Stage 9 amendment

- [x] `docs/spec.md` §4.7: section header + label + «Проверить обновления» button; dev-build check semantics (dev ≈ nightly build, compared by `commit_sha`, may switch to stable); row visibility is gated on discovery only (the interval no longer hides the row); one check in flight — a press made while a check runs stays pending and runs when the slot frees; Apply re-arms/clears the interval deadline
- [x] `docs/spec.md` §10: locale count 92 → 94 (`general.update_check_label` dropped as dead wording), test-count line refreshed to the post-Stage-9 numbers (549 юнит passed / 2 ignored, e2e 9 passed / 1 ignored), release size measured on the Stage 9 commit (< 12 МБ)
- [x] `docs/spec.md`: note the GUI-failure park path arms its own startup check (a second fetch on that path) — pre-existing behavior, documented rather than changed
- [x] `docs/spec.md`: park re-arm timing differs from GUI (park re-arms on spawn, GUI after a drained scheduled outcome) and the abandoned-restart park path may fetch once more — both documented as behavior, not defects
- [x] `docs/spec.md` §10: CI verification deferred to the PR the user opens (push is unavailable here); no green-CI claim anywhere in the spec
- [x] Gates: `./scripts/check_full.sh`, `./cargo.sh test --workspace`, release size re-measure (< 12 MB) — exe 11 448 320 B, Linux 18 649 840 B, measured on `f5d210c` on one dev-host

**Criterion:** product spec describes the shipped General-tab update section, the forced check, and dev-build behavior; gates green.
**Commit:** `docs(spec): align §4.7 and §10 with the shipped update section, counts, and sizes`

---

## Stage 11 — Forced-check status, date/version comparison, stale-status reset

- [x] `src/update.rs`: RFC 3339 date reader (own parser, no new dep) + numeric `major.minor.patch` version compare; `decide_update` returns true only when release date is newer than build time OR release version is higher — `commit_sha` no longer triggers (release and `-dev` builds share the rule); `BuildInfo` gains build time from `build::BUILD_TIME_3339`; unparseable date → Parse error
- [x] `src/update.tests.rs` (TDD red first): nightly older than local build → false; release newer date → true; higher version → true; same date+version → false; dev build newer than nightly → false; date parse rejects malformed input
- [x] `src/gui.rs`: forced-check result status painted to the right of «Проверить обновления» — red `UpdateError` text on failure, normal «Обновлений нет» on up-to-date, nothing while a check is in flight; pure helpers for the status slot so it is unit-testable
- [x] `src/gui.rs`: any check outcome replaces the previous result — up-to-date and fetch failure clear a held discovery (switching channel to stable and hitting 404 no longer keeps «Доступно обновление»); install failure still keeps discovery, successful install clears it
- [x] `src/gui.tests.rs` (TDD): status-slot tests (failure→red text, up-to-date→normal text, in-flight→no status) + stale-discovery-clear tests + the existing slot/schedule tests stay green
- [x] `locales/app.yml`: add `update.no_updates_available` (ru «Обновлений нет») across en/ru/zh-CN/es/ja/ko; `src/i18n.tests.rs` pins 94 → 95
- [x] `docs/spec.md` §4.7/§10: comparison rule (date OR version, sha no longer a trigger), status text next to the button, stale-status reset, counts (95 keys, new test totals), dev-build wording
- [x] Gates: `./scripts/check_full.sh`, `./cargo.sh test --workspace`, `./cargo.sh clippy --workspace --all-targets -- -D warnings`, `./cargo.sh check --target x86_64-pc-windows-gnu`, `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`, release exe size re-measure (< 12 МБ)

**Criterion:** a nightly check on a newer local build reports «Обновлений нет» next to the button; a forced check that fails shows the error text in red and clears a previously held «Доступно обновление»; comparison no longer depends on `commit_sha`; gates green and release size under budget.
**Commit:** `5ee4393 feat(update): compare release date and version, and report the check result beside the button`
