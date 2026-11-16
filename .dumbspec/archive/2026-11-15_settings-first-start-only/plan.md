# 2026-11-15_settings-first-start-only: implementation plan (settings window only on first launch)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan
- [x] Capture draft.md (raw user request, Russian)
- [x] Branch `2026-11-15_settings-first-start-only` created from clean master
- [x] Skeleton spec from draft; user confirmed: proceed to research as-is
- [x] Research subagent report → research.md (config.rs load_or_create facts, unconditional gui::run at main.rs:544, tray reopen path, docs/spec.md §2:125 «скрытый запуск»)
- [x] Spec v1 (Created header + Changes), open questions resolved → none
- [x] plan.md written

**Criterion:** draft.md, research.md, spec.md (v1), plan.md exist in `.dumbspec/current/2026-11-15_settings-first-start-only/`; spec has no blocking questions.
**Commit:** docs(dumbspec): settings-first-start-only task — draft, research, spec v1, plan

## Stage 1 — config: first-run signal from `load_or_create`
- [x] Red: in `src/config.tests.rs` extend/adjust `load_or_create_*` tests for the new signature — missing file returns `(Settings::default(), created=true)` and creates the file; valid file → `(parsed, false)`; corrupt file → `(defaults, false)` and no rewrite; parent-dir test updated.
- [x] Green: change `config::load_or_create` (src/config.rs:165) to return `(Settings, bool)` where the bool = "default file was created because it did not exist"; keep existing logging and corrupt-file no-rewrite semantics.
- [x] Update callers: `run_resident` (main.rs:429) keeps the flag; `run_shot` (main.rs:329) discards it (`let (settings, _)`).
- [x] Refactor; run `./scripts/check.sh` (unit tests green, clippy clean).

**Criterion:** `./cargo.sh test` — config.tests.rs passes with the new signature; `load_or_create` returns `created=true` only when the file was absent before startup and was written with defaults; corrupt file → `created=false`.
**Acceptance:** independent verifier PASS 6/6 (only config.rs/config.tests.rs/main.rs touched; 4 load_or_create tests pass; SKIP_E2E gate green; flag computed as `!path.exists()` sharing the one condition with `create_default_file`). Nuance for Stage 2: flag means "config was missing at startup" (creation-failure still reports true) — acceptable: a missing config is exactly the first-run case.
**Commit:** feat(config): report first-run default-config creation from load_or_create

## Stage 2 — startup wiring: settings window visible only on first run
- [x] Red/verify: startup decision covered by the Stage 1 signal (decision = `created`); no extra pure function (KISS per spec).
- [x] Green: `gui::run` (src/gui/window.rs:140) takes a `visible: bool` parameter; root `ViewportBuilder` (window.rs:153–176) gets `.with_visible(visible)`.
- [x] Wire: `run_resident` (main.rs) passes the config `created` flag into `gui::run` (single call site, main.rs:544); the `Settings window opening` INFO log reflects whether the window opens visible or hidden-to-tray.
- [x] Compile gate for Windows target: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`.
- [x] `./scripts/check.sh` green (Linux unit tests + clippy -D warnings); confirm hidden startup keeps OverlayHost/ToastService/tray reopen intact (they stay in the create closure, no skip of gui::run).

**Criterion:** Linux run: `gui::run` called with `visible=false` when config exists, `visible=true` when the config was just created; test binaries compile for x86_64-pc-windows-gnu; existing app wiring (toast handoff, tray tick) untouched.
**Acceptance:** independent verifier PASS 7/7 — 12th param `visible: bool`, `.with_visible(visible)` at window.rs:178 (egui 0.37 supports it), sole call site passes `first_run`, tray ShowSettings → `Visible(true)` untouched, create-closure wiring intact, Linux gate green, Windows test binaries compile.
**Commit:** feat(main): show settings window only on first launch, hidden-to-tray otherwise

## Stage 3 — docs + full gate
- [x] docs/spec.md §4.5: settings window opens at first launch; subsequent launches start hidden to tray (open via tray left-click / menu).
- [x] Full gate at stage boundary: `./scripts/check.sh` (with e2e included) + `./scripts/check_full.sh` (fmt + fmt --check).
- [x] Report to user; ask for confirmation before archiving (never inferred).

**Criterion:** docs/spec.md states the startup visibility rule; full gate (incl. e2e compile, fmt --check) passes.
**Acceptance:** independent verifier PASS — bullet at docs/spec.md:631-635 covers first-launch visible / hidden subsequent / tray reopen / corrupt-config exception, consistent with §7 and §4.4; full `./scripts/check.sh` exit 0 (617 unit + e2e target ran: 9 passed, 1 ignored desktop-only success test); `./scripts/check_full.sh` exit 0, no fmt drift.
**Commit:** docs(spec): settings window opens only on first launch

## Stage 4 — suppress eframe first-frame reveal (user-reported regression)
- [x] Research finding recorded (research.md): eframe 0.37 `post_rendering` shows the root after the first paint regardless of `with_visible(false)`; logic runs before paint per frame.
- [x] `SettingsApp` gains `startup_hidden: bool` state threaded from `gui::run`'s `visible` (hidden iff `!visible`); while it is active every logic pass sends `ViewportCommand::Visible(false)`.
- [x] Tray `ShowSettings` clears `startup_hidden` and keeps today's `Visible(true)`; `Exit` behavior unchanged.
- [x] Unit test in `src/gui/app.tests.rs` (harness wired like `gui::run`): hidden-start app emits `Visible(false)` suppression and a tray ShowSettings flips it to shown; run `SKIP_E2E=1 ./scripts/check.sh` + `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`.

**Criterion:** with existing config the app keeps sending `Visible(false)` until tray ShowSettings (unit test green); gates green; Windows test binaries compile. Visual acceptance on the user's Windows host (rebuild from this branch): first run opens the window; subsequent runs stay hidden, tray click opens.
**Acceptance:** independent verifier PASS — suppression per logic pass (`app.rs:619-621`, placed after the tray poll so a same-pass ShowSettings emits only `Visible(true)`), `show_from_tray` (:370-374) clears the flag, 14th param threaded via `!visible` (window.rs:247-252); 3 new tests read `LogicOutput::viewport_commands` (ROOT): re-send pinned in passes 1+2, show flips to Visible(true)+zero later, visible startup emits none; `SKIP_E2E=1 ./scripts/check.sh` exit 0 (620 unit pass), all-target clippy exit 0, Windows test binaries link (3 exes). Deviation noted: .dumbspec artifacts also modified (this stage's journal) — included in the commit. Windows visual acceptance remains on the user's rig.
**Commit:** fix(gui): keep hidden startup hidden across eframe first-frame reveal

## Stage 5 — settings window spawned from the tray (no flash, v3)
- [x] Red unit tests in `src/gui/app.tests.rs`: hidden startup registers child viewport `virshot-settings` (root commands include `RequestPaintWhileHidden`, settings child never gets `Visible(true)` on a hidden startup); tray ShowSettings sends `Visible(true)` to the settings child (once) and a later ShowSettings sends `Focus`; root `ui` draws no settings surface.
- [x] Green: settings UI (tabs + bottom bar + capture/pad press handlers) moves from root `ui` into `show_viewport_deferred("virshot-settings")` registered while the window is open — constant builder (title/size/icon/chrome as today, `.with_visible(false)`, opaque CentralPanel like about.rs), X recorded inside child paint → hide-to-tray (`Visible(false)` + draft revert); hidden-child sequencing per the chip recipe (re-send `RequestPaintWhileHidden` to ROOT while the child has no window/paint, child `RequestPaintWhileHidden` until first paint, then `Visible(true)` iff first-run; throttle per pass guarded by `raw.viewports.contains_key`).
- [x] Wiring: root `ui` becomes empty; root `startup_hidden` suppression stays (empty root, invisible flash); `gui::run` root builder always `.with_visible(false)`, `first_run` becomes `open_settings_on_start` (main.rs log lines updated); tray Exit/OK/Cancel exit semantics unchanged.
- [x] docs/spec.md §4.5: последующие запуски — окно не существует до открытия из трея; X окна = скрытие в трей (программа остаётся в трее); первый запуск — окно открывается сразу.
- [x] Refactor + gates: `SKIP_E2E=1 ./scripts/check.sh`, `./cargo.sh clippy --all-targets -- -D warnings`, `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`, `./scripts/check_full.sh`.

**Acceptance:** independent verifier PASS at 60caf03: settings UI lives in a Deferred CHILD viewport registered on every logic pass with a constant builder (locale title changes map to `ViewportCommand::Title`, no OS-window recreation), ROOT never `Visible(true)`, first-run reveal one-shot (`settings_revealed`), tray show sends `Visible(true)` once then `Focus`, X hides the child and reverts the draft. Minor findings fixed in 4ab1623 (stale comments in main.rs/app.rs; new regression test `settings_child_close_hides_to_tray_and_reverts_draft` pins the child X-close path — hidden ≠ destroyed, no `CancelClose` on root). Gates: `./cargo.sh test --bin virshot` 621 pass, clippy `--all-targets -D warnings` exit 0, Windows-gnu `test --no-run` exit 0, `./scripts/check_full.sh` exit 0. Windows visual acceptance (no flash, tray-first) remains on the user's rig.
**Commit:** feat(gui): settings window spawned from the tray, root stays hidden; test(gui): pin settings child X-close; refresh stale startup/logic comments

## Stage 6 — visually inert root window (kills the startup flash, v4)
- [x] Red unit test in `src/gui/window.rs` sibling tests: pure `root_viewport_builder()` returns a visually inert builder — `decorations == Some(false)`, tiny `inner_size` (1×1), `visible == Some(false)`, `transparent == Some(true)` (egui 0.37 exposes these as pub fields, not accessors); today's builder failed it (E0425 before extraction).
- [x] Green: `pub(crate) fn root_viewport_builder() -> egui::ViewportBuilder` — `.with_title(...).with_decorations(false).with_inner_size([1.0, 1.0]).with_transparent(true).with_visible(false)`, icon handling kept; settings-window-era minimize/maximize flags dropped (meaningless without decorations); v3 child machine untouched. Children inherit only `icon` + `transparent` (glow_integration.rs:1618-1637), decorations come from the child's own builder — About/chips keep their chrome.
- [x] Comments + log wording: root described as inert (not merely hidden); suppression documented as a safety net; docs/spec.md §4.5 unchanged.
- [x] Gates: `./cargo.sh test --bin virshot` 622 pass, `./cargo.sh clippy --all-targets -- -D warnings` exit 0, `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` exit 0, `./scripts/check_full.sh` exit 0.

**Criterion:** root builder pinned as inert by a unit test; unit suite green; Windows cross-compile green. Windows visual acceptance (no startup flash on fresh-config and existing-config runs) remains on the user's rig.
**Acceptance:** independent verifier PASS at 53dd270 — `root_viewport_builder()` pinned visually inert by `root_viewport_builder_is_visually_inert` (decorations `Some(false)`, inner_size `Some(1×1)`, transparent `Some(true)`, visible `Some(false)`), used by `gui::run` with icon kept; v3 settings child + About/update/chip chrome untouched (children inherit only icon+transparent, glow_integration.rs:1618-1636); suppression kept as safety net; exit paths via `ViewportCommand::Close` intact. Minor findings fixed in e2428fd: `.with_taskbar(false)` (eframe reveals root on first frame → without it the one-frame blip registers in taskbar/Alt-Tab; chips use the same guard), stale root-viewport docs in gui/mod.rs + app.rs header rewritten, reveal/restore citations corrected to epi_integration.rs:443-448 / :45. Gates: `./cargo.sh test --bin virshot` 622 pass, clippy `--all-targets -D warnings` exit 0, Windows-gnu `test --no-run` exit 0. Windows visual acceptance (no startup flash, no taskbar blip) remains on the user's rig.
**Commit:** fix(gui): make the root window inert so startup reveal shows nothing; fix(gui): keep the inert root out of the taskbar; refresh stale viewport docs

## Stage 7 — no console flash from the liveness probe (v5)
- [x] Red unit tests in `src/instance.tests.rs` on pure helpers (extractable so the Linux host can run them): `tasklist_args(pid)` pins `["/FI", "PID eq <pid>"]`; `create_no_window_flag()` pins `0x0800_0000` (Win32 `CREATE_NO_WINDOW`). Today the args are inline in the `cfg(windows)` probe and the flag does not exist → tests fail (3 × E0425).
- [x] Green: `cfg(windows)` `probe_pid_alive` builds `Command::new("tasklist")` from `tasklist_args(pid)` and applies `CommandExt::creation_flags(create_no_window_flag())` (safe std API) so the child console process gets no console window; comment states why (GUI-subsystem parent → child console flash). Liveness semantics unchanged: uncertain probe still honours the lock. Helpers gated `#[cfg(any(test, windows))]` so Linux non-test builds have no dead code.
- [x] No other runtime spawn needs the flag (`current_exe()` restart/update re-exec is the GUI-subsystem binary itself) — verified, noted in the comment.
- [x] Gates: `./cargo.sh test --bin virshot` 624 pass, `./cargo.sh clippy --all-targets -- -D warnings` exit 0, `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` exit 0, `./scripts/check_full.sh` exit 0.

**Criterion:** probe args + flag pinned by unit tests; suite green; Windows cross-compile green. Windows hardware acceptance (second launch shows no console flash, exits with the AlreadyRunning error in the log) remains on the user's rig.
**Acceptance:** independent verifier PASS at 3e63874 — `tasklist_args` pinned to `["/FI", "PID eq <pid>"]` and `create_no_window_flag()` pinned to `0x0800_0000` by 2 new tests; the `cfg(windows)` probe spawns `tasklist` with `creation_flags(CREATE_NO_WINDOW)` via safe std (no unsafe, no new deps) with unchanged liveness semantics — probe runs only on the existing-lock path, so the first launch never spawns it; `take_over_or_report`/`replace_stale_lock`/`Drop` untouched. Gates: 624 unit tests pass, clippy `--all-targets -D warnings` exit 0, Windows-gnu `test --no-run` exit 0. Windows hardware acceptance (no flash on duplicate launch) remains on the user's rig.
**Commit:** fix(instance): run the pid probe without allocating a console window
