# 2026-11-13_gpu-repaint-throttle: implementation plan (GPU repaint throttle)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan
- [x] Capture raw input into `draft.md` (interval clarified: 200 ms)
- [x] Superficial research pass → `research.md` (16 repaint call sites, versions, constant
  convention, mechanism hazards, gates)
- [x] `spec.md` v1 + review decisions (fade stays immediate; About/dialog throttled;
  examples moved into scope)

**Criterion:** `draft.md`, `research.md`, `spec.md` exist in `current/2026-11-13_gpu-repaint-throttle/`
and every call site / constraint cited in `research.md` carries `file:line`.
**Commit:** `docs(spec): add gpu repaint-throttle task (draft, research, v1 spec)`

## Stage 1 — Deep research: egui 0.37 repaint API semantics
- [x] Verified `egui::Context::request_repaint_after` semantics (cratedoc + locked sources):
  earliest delay wins (strict `<`), ZERO ⇒ two repaints, non-zero ⇒ one, no egui floor;
  eframe floors invisible windows at `now + 100ms`
- [x] Per-viewport DELAYED repaint exists: `Context::request_repaint_after_for(delay, id)`
- [x] Root repaint does NOT wake Deferred children → `drain(Redraw)` must request the concrete
  viewport immediately
- [x] Assertion path: `set_repaint_observer` (sees every request) + `has_requested_repaint_for`
  (bool); no numeric "time until repaint" accessor
- [x] Facts appended to `research.md` ("Verified egui 0.37 repaint API"); mechanism recorded in
  `spec.md` (Changes entry)

**Criterion:** `research.md` contains API facts backed by cratedoc/source citations (not memory),
and `spec.md` states which mechanism is used for per-viewport throttling.
**Commit:** `docs(research): pin egui 0.37 repaint-after semantics for the throttle`

## Stage 2 — Overlay host: throttled steady-state repaint
(Coupled steps of one file/module are executed as grouped sub-items: red test → implementation
→ comment/refactor+gates, one subagent per group.)
- [x] Red test in `crates/overlay/src/host.rs` inline `#[cfg(test)]` (helpers at `:504-528`,
  tests at `:710`): a live chip with no state change must end the pass with an EFFECTIVE
  `FullOutput.viewport_output[id].repaint_delay == REPAINT_INTERVAL` (chip + root), while
  spawn/redraw end with ZERO. (Added as RED: 2 failing tests at `:1438` and `:1502`; the
  observer variant was replaced by the FullOutput assertion because
  `send_viewport_cmd_to` pins `repaint_delay = ZERO` and dedups the delayed request.)
- [x] Add `REPAINT_INTERVAL: Duration = Duration::from_millis(200)` module const with doc
  comment (convention precedent `src/gui/app.rs:100-104`)
- [x] Gate viewport-command sends by change: keep the last sent OuterPosition / InnerSize /
  Visible / ContentProtected per chip and send only when the value differs — otherwise every
  pass pins `repaint_delay = ZERO` and the throttle is ineffective
- [x] Send `ViewportCommand::RequestPaintWhileHidden` to ROOT only while the root window is
  hidden and the chip needs painting, not every pass
- [x] `OverlayHost::paint` (`crates/overlay/src/host.rs:471`): `request_repaint_of(id)` →
  `ctx.request_repaint_after_for(REPAINT_INTERVAL, id)`, guarded by the window-present check
  (`:246-250`); re-issued every pass so a steady chip stays alive at ~5 fps (a delayed request
  gives exactly one repaint, and re-registration keeps Deferred children retained)
- [x] `OverlayHost::paint` root branch (`:479-483`): `ctx.request_repaint()` →
  `ctx.request_repaint_after(REPAINT_INTERVAL)`
- [x] Keep immediate repaint where required: `OverlayCommand::Spawn` (`:251`) stays immediate;
  `OverlayCommand::Redraw` (`:269`) additionally requests the concrete chip viewport immediately
  (root wake does not reach Deferred children), guarded by window presence
- [x] Update the stale debug comment at `host.rs:424-431` ("a steady chip would otherwise write
  one line every repaint") to the new cadence
- [x] Refactor; `./cargo.sh test -p overlay` and `./scripts/check.sh` green

**Criterion:** `./cargo.sh test -p overlay` passes with the new steady-state test green; grep shows
no unconditional `request_repaint` inside `OverlayHost::paint` and the interval appears only via
`REPAINT_INTERVAL`.
**Commit:** `perf(overlay): throttle steady chip repaint to REPAINT_INTERVAL`

## Stage 3 — Settings window: About + update dialog child viewports
- [x] Red test in `src/gui/app.tests.rs`: while About/dialog is open with unchanged content, the
  logic pass must schedule a delayed repaint, not an immediate one
- [x] Add the owning module const (`src/gui/app.rs`, alongside `TRAY_POLL_TICK` at `:104`) and
  use `ctx.request_repaint_after_for(interval, id)` — Stage 1 confirmed the per-viewport variant
  exists (`egui context.rs:2222`)
- [x] `src/gui/app.rs:571` (About) and `src/gui/app.rs:844` (update dialog): replace the per-pass
  `request_repaint_of` with the throttled request
- [x] Preserve immediate repaint for user-driven transitions in the same viewports
  (open/close, button press, notes arrival — `:667`, `:774`, `:505`)
- [x] Do not touch `TRAY_POLL_TICK` scheduling (`:954`, `:960`); verify hidden-window logic passes
  still run (200 ms tick is shorter than 500 ms, so tray polling and update deadlines stay)
- [x] Refactor; `./scripts/check.sh` green

**Criterion:** `./scripts/check.sh` green; no per-pass `request_repaint_of` for static-content
child viewports; interval comes from a named const.
**Commit:** `perf(gui): throttle About and update-dialog repaint`

## Stage 4 — Examples: named repaint intervals
- [x] Red/behaviour test or assertion for `crates/icons/examples/icons_gallery.rs`: interval comes
  from the existing `REPAINT_EVERY` const (`:48`) and the gallery still closes after
  `MAX_FRAMES` (`:35`) — keep it explicit rather than duplicating the literal. Value **kept at
  100 ms**: the run is bounded by frames (`MAX_FRAMES` = 10 paints either way), not by wall clock,
  so aligning to 200 ms paints the same frames and only stretches the run ~1 s → ~2 s; nothing in
  the gallery animates (static texture rows + labels). 100 ms is also eframe's invisible-window
  floor, so it is the minimum useful interval.
- [x] `crates/video-capture/examples/capture_demo.rs`: replaced the unconditional
  `request_repaint()` with `ctx.request_repaint_after(REPAINT_INTERVAL)` (`REPAINT_INTERVAL` =
  200 ms, module const `:104`); the ack-polling comment now states that acks are polled in this
  pass and are therefore drawn ≤ 200 ms later, not the moment they arrive
- [x] Examples-only changes: no shipped-app code touched (`src/`, `crates/overlay/src/host.rs`,
  `crates/toast/` untouched)
- [x] Gates green: `./cargo.sh check --all-targets` (root package) **and**
  `./cargo.sh check --all-targets --workspace` — plain `--all-targets` at the root selects the
  `virshot` package only, so the crate examples are compiled by the workspace run; example tests
  run with `./cargo.sh test --examples -p icons -p video-capture` (4 tests green)

**Criterion:** both examples compile under `--all-targets` and use named consts for the repaint
interval; no literal repaint interval left in the examples.
**Commit:** `perf(examples): name repaint intervals in gallery and capture demo`

## Stage 5 — Boundary gates
- [x] `./cargo.sh test --workspace` (overlay + toast + gamepad compile/run)
- [x] `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` (Windows test binaries compile)
- [x] `./scripts/check_full.sh` (fmt-aware gate)
- [x] Record what was and was not measured (headless Linux sandbox — no real GPU numbers) in
  `spec.md` Changes

**Criterion:** all four gate commands exit 0; the honest measurement limitation is recorded.
**Commit:** N/A
