# 2026-11-03_tests-backlog: implementation plan (tests layout, UI-verify script, profiling workflow)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = failing check first → implementation to green → refactor + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan

- [x] Draft captured from backlog (tests section) into `draft.md`
- [x] Initial spec written from draft; refinement gate confirmed by user
- [x] Research R1 (test layout, visibility, gates), R2/R2b/R2c (qa-harness, Xvfb, window capture), R3/R3b (perf, valgrind, flamegraph) appended to `research.md`
- [x] Spec v1 written (Created + Changes); open questions resolved by user decisions (sibling test files, no WM, debug binary, no SVG tool, one script)
- [x] `plan.md` written

**Criterion:** `draft.md`, `research.md`, `spec.md` (v1 format), `plan.md` exist in `current/2026-11-03_tests-backlog/`; scope decisions recorded in spec Changes.
**Commit:** `docs(dumbspec): add tests-backlog task (draft, research, spec v1, plan)`

## Stage 1 — Unit tests moved to sibling files

- [x] Failing baseline: `grep -c '#\[cfg(test)\] mod tests' src/*.rs` = 10 (bodies inside module files) — target is 0 bodies
- [x] `src/template.tests.rs` — move `template.rs` test body verbatim; hook in module: `#[cfg(test)] #[path = "template.tests.rs"] mod tests;`; run `./scripts/check.sh`
- [x] `src/config.tests.rs` — same pattern (`config.rs`, 46 tests)
- [x] `src/bindings.tests.rs` — same pattern (21 tests)
- [x] `src/logging.tests.rs` — same pattern (8 tests)
- [x] `src/instance.tests.rs` — same pattern (9 tests)
- [x] `src/tray.tests.rs` — same pattern (7 tests)
- [x] `src/overlay.tests.rs` — same pattern (21 tests)
- [x] `src/hotkeys.tests.rs` — same pattern (17 tests, keeps the `cfg(test)` mock `Registrar`)
- [x] `src/gui.tests.rs` — same pattern (49 tests, egui types)
- [x] `src/capture.tests.rs` — same pattern (22 tests, xcap types)
- [x] Refactor: module files keep only the 2-line hook; no visibility widening, no `src/lib.rs`
- [x] Windows cross-compile: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`

**Criterion:** `./scripts/check.sh` green; `cargo test` total still 232 unit tests (231 pass + 1 ignored) + 9 E2E pass + 1 ignored; each `src/<module>.rs` has no `mod tests` body (hook only); windows-gnu `test --no-run` compiles; `./scripts/check_full.sh` clean.
**Commit:** `refactor(tests): move module unit tests to sibling src/*.tests.rs files`

## Stage 2 — `scripts/ui-verify.sh` (see the app under Xvfb)

- [x] Failing check first: run `./scripts/ui-verify.sh all` — script absent → exit non-zero
- [x] Script skeleton: one bash invocation, subcommands `start | shot | window | tray | log | stop | all`; scratch dir `./.tmp/ui-verify/<ts>/`; `trap` cleanup EXIT/INT/TERM + kill tree; never `windowkill`
- [x] Display: `env -u WAYLAND_DISPLAY xvfb-run -a --server-args="-screen 0 1280x1024x24"`; app runs inside `dbus-run-session --` (tray registers on the session bus); everything (app + capture + input + dbus calls) inside the same invocation
- [x] Isolated run env: config in scratch dir with `[general] save_path = "<scratch>/out"`, log dir + state dir in scratch; remove `<config>.lock` at cleanup
- [x] `start`: launch resident `./target/debug/virshot --config …` in background; poll `xdotool search --name virshot` (0.1 s steps, timeout) → report window id + `xwininfo -id` geometry
- [x] `window`: capture window evidence — `import -window <id>` (+ `scrot -w <id>`), assert non-blank via unique-color count; also `scrot -o` root shot and `convert -crop` region check (no WM → absolute == relative origin; with `--wm` crop uses **absolute** origin)
- [x] `shot`: input `xdotool windowfocus --sync <id>` + `xdotool key Print`, then grep the log for `Hotkey registered`, `Saved:`, `Toast shown`; xcap `virshot shot` path retried (≤3 attempts) since it is ~25–40 % reliable; hotkey assertions only in the no-WM configuration
- [x] `tray`: D-Bus evidence — bus name `org.kde.StatusNotifierItem-<pid>-1` present, log `System tray ready`, `ToolTip`/`IconPixmap` (32×32 RGBA → PNG via ImageMagick), `/MenuBar` `GetLayout` labels (`Take screenshot`, `Settings`, `Exit`), `Activate` re-shows a hidden window, menu `Event` Settings/Exit behavior (Exit = clean exit + lock removed)
- [x] `--wm` (optional, openbox): asserts `wmctrl -l/-m` rc 0 and `virshot shot --window` rc 0; no hotkey assertions (registration fails under a WM — unpinned, see research R2d)
- [x] `log`: dump the run log (ERROR/WARN lines summarized)
- [x] `stop`: SIGTERM, wait ≤2 s, `rm <config>.lock`
- [x] Usage documented in the script header (options: `--config`, `--wm`, `--keep`, `--timeout`; resident = bare `virshot`, no `--res` flag exists); `./.tmp/` artifacts printed as paths; deviations documented (dbusmenu `Event` key must be `"clicked"` — ksni ignores `"left"`; Activate Map-State toggle assertable only under `--wm`)
- [x] Refactor: shared helpers only inside the script; no new deps (ImageMagick + xdotool + scrot + dbus tools + openbox/tint2, all user-installed)

**Criterion (met):** `./scripts/ui-verify.sh all` exits 0 in one call and leaves in `./.tmp/ui-verify/<ts>/`: a window PNG with >100 unique colors, a root PNG, a tray icon PNG (32×32), and a log containing `Hotkey registered: PrintScreen`, `System tray ready`, `Saved:`; `/MenuBar` layout shows the three labels; no `virshot`/`Xvfb` processes and no scratch dirs left after the run.
**Commit:** `feat(scripts): add ui-verify.sh manual UI check under Xvfb`

## Stage 3 — Profiling workflow (`scripts/prof.sh`)

- [x] Failing check first: run `./scripts/prof.sh heap` — script absent → non-zero
- [x] `heap`: `valgrind --tool=massif --time-unit=ms --massif-out-file=./.tmp/prof/<ts>/massif.out ./target/debug/virshot shot …` on the headless/looped path (isolated config, retry for xcap flakiness); print peak `mem_heap_B` from the report text (`msprint` is absent → parse the file)
- [x] `leak`: `valgrind --tool=memcheck --error-exitcode=1 --leak-check=full` on `--help`/`--version`/headless `shot` (`virshot doctor` does not exist); print heap usage + LEAK SUMMARY + ERROR SUMMARY
- [x] `cpu`: `perf record -F 99 --call-graph dwarf,64000 -o ./.tmp/prof/<ts>/perf.data -- <resident run>` (looped `virshot shot` fallback), then `perf report --stdio` head + `perf script | head`; no SVG (no flamegraph/inferno installed)
- [x] `mem-count`: `perf stat -e mem_inst_retired.all_loads,mem_inst_retired.all_stores -- <looped shots>` (counting works; `perf mem`/sampling does not on this WSL2 PMU)
- [x] Cleanup: scratch under `./.tmp/prof/`, `rm <config>.lock` after forced SIGTERM; artifacts kept for inspection, scratch removed on `--clean`
- [x] Header comments: what each subcommand produces and how to read it; no `perf mem`, no release binary (debug has symbols)
- [x] Refactor: single script, shared scratch helper; nothing installed

**Criterion:** `./scripts/prof.sh heap` writes a massif file under `./.tmp/prof/` and prints a peak heap number; `./scripts/prof.sh leak` exits 0 with a LEAK SUMMARY line; `./scripts/prof.sh cpu` produces `perf.data` + a `perf report --stdio` excerpt with resolved symbols; `mem-count` prints nonzero `mem_inst_retired.all_loads` counts.
**Commit:** `feat(scripts): add perf/valgrind profiling workflow scripts`

## Stage 4 — Regression + backlog note

- [x] `./scripts/check.sh` green (linux) and `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` compiles
- [x] `./scripts/check_full.sh` green (fmt gate)
- [x] `./scripts/ui-verify.sh all` + `./scripts/prof.sh heap|leak|cpu|mem-count` all exit 0
- [x] Backlog note in plan/spec: tests-section backlog items now covered by Stages 1–3 (no `docs/spec.md` product change)

**Criterion (met):** both gates green and all helper scripts exit 0 in one invocation each; scratch artifacts confined to `./.tmp/`.
**Commit:** `chore(qa): verify ui-verify and prof scripts on linux + windows-gnu compile gate`


## Backlog coverage note

- `docs/spec.md` tests-backlog item 1 (tests out of module files) — done in Stage 1: sibling
- Item 2 (manual UI observation) — done in Stage 2 as helper scripts (`scripts/ui-verify.sh`),
- Item 3 (memory profiling workflow) — done in Stage 3 (`scripts/prof.sh`):
- No product change in `docs/spec.md`.
