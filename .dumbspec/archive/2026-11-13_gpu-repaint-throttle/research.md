# Research — GPU repaint throttle (supervicial pass, 2026-11-13)

Facts only, every finding cites `file:line`. No proposals.

## Call sites

### `request_repaint()` (unconditional, immediate)

| # | Location | Trigger |
|---|----------|---------|
| 1 | `src/gui/app.rs:449` (in `request_window_exit`) | Exit from OK/Cancel button: sets `close_for_exit`, sends `ViewportCommand::Close`, then repaints so the close decision runs this frame. Event-driven. |
| 2 | `src/gui/app.rs:471` (tray `ShowSettings`) | Tray left-click/`Settings`: sends `Visible(true)` + repaint to re-show the window. Event-driven. |
| 3 | `src/gui/app.rs:505` (tray `About`) | Tray `About` opens/raises the About child viewport; repaint so registration/Focus lands. Event-driven. |
| 4 | `src/gui/app.rs:510` (tray `Exit`) | Tray `Exit`: `exit_requested` + `ViewportCommand::Close` + repaint. Event-driven. |
| 5 | `src/gui/app.rs:667` | New update discovery drained from the check mailbox (`discovery` is `Some`): status row/chip/dialog state changed → repaint. Event-driven (background thread result arriving). |
| 6 | `src/gui/app.rs:774` | Install-thread outcome drained (`install_rx.try_recv()`): status transition (Ok → restart close, Err → danger chip). Event-driven. |
| 7 | `src/hotkeys.rs:324` (in `record_probe_press`, helper doc at `:316`) | Bind-probe press recorded on the hotkey listener thread (Windows keys Windows never delivers to egui, spec §7 price note): wakes the GUI loop so the bind row updates while hidden. Event-driven, cross-thread wake. Attached via `HotkeysController::attach_probe_wake` (`src/hotkeys.rs:629-630`, field `wake: Option<Context>` at `:285`). |
| 8 | `crates/overlay/src/host.rs:251` (`OverlayHost::drain`, `OverlayCommand::Spawn`) | A toast chip spawns: repaint the host loop so `paint` registers the viewport on this pass. Comment at `:246-250` records a hard mechanism fact: `request_repaint_of` on a window-less viewport pins its `repaint_delay` to ZERO and later requests get deduplicated — the window then never redraws. Event-driven. |
| 9 | `crates/overlay/src/host.rs:269` (`drain`, `OverlayCommand::Redraw`) | Toast fade/content update arrives from the timer thread: host-loop repaint; the per-viewport request is deferred to `paint` (comment `:267-268`). Event-driven (cross-thread wake, 25 ms cadence during fade — see below). |
| 10 | `crates/overlay/src/host.rs:471` (`OverlayHost::paint`, per live viewport) | **Unconditional per-pass**: `ctx.request_repaint_of(viewport)` for every overlay viewport that already has a window — a steady chip with unchanged content is still re-requested every pass. Debug comment at `:424-431` confirms ("a steady chip would otherwise write one line every repaint"). |
| 11 | `crates/overlay/src/host.rs:479` (`paint`, `if has_live`) | **Unconditional per-pass while any overlay is alive**: `ctx.request_repaint()` + `RequestPaintWhileHidden` to ROOT (`:480-483`) so a hidden settings window still runs passes (chips spawn/paint/fade while hidden). Because paint re-requests every pass, a live chip keeps the root loop repainting continuously, superseding the 500 ms tick. |
| 12 | `crates/toast/src/service.rs:295` (`request_wake`, doc at `:98-100`) | Toast timer thread wakes the host `egui::Context` after touching windows: on Show (`:195`), Dismiss (`:199`), fade Tick (`:213`), Gone/hide (`:208`). During `FadeOut` this fires every `FADE_TICK_MS` = 25 ms (`crates/toast/src/rules.rs:30`, `next_deadline` `:125-127`). |
| 13 | `crates/video-capture/examples/capture_demo.rs:1056` (`App::ui`) | **Unconditional per-frame** in the demo: acks arrive only from the capture thread when a frame lands, so the UI must keep repainting to poll them (comment `:1052-1055`). Example-only. |

### `request_repaint_after(...)` (already throttled — precedents)

| # | Location | Snippet / trigger |
|---|----------|-------------------|
| 14 | `src/gui/app.rs:954` | `ctx.request_repaint_after(TRAY_POLL_TICK);` — tray events arrive on crossbeam receivers and never wake winit (comment `:950-953`), so the logic pass schedules the next tick (500 ms). |
| 15 | `src/gui/app.rs:960` | `ctx.request_repaint_after(deadline.saturating_duration_since(now).min(TRAY_POLL_TICK));` — auto-update check deadline needs a pass at the deadline instant; `egui` keeps the earliest scheduled repaint, so this is additive to #14 (comment `:955-958`). |
| 16 | `crates/icons/examples/icons_gallery.rs:123` | `ui.ctx().request_repaint_after(REPAINT_EVERY);` — example: frame-count-driven gallery, constant at `:38` (100 ms), close after `MAX_FRAMES` = 10 (`:35`). Precedent for a named repaint-interval constant. |

### `request_repaint_of(...)`

| # | Location | Trigger |
|---|----------|---------|
| 17 | `src/gui/app.rs:571` | About child viewport: re-requested every logic pass while registered/open — static content (changelog markdown + icon), re-request is per pass, not per change. |
| 18 | `src/gui/app.rs:844` | Update dialog child viewport: "A hidden settings window still repaints this viewport on its own schedule; nudge it so notes arrival lands promptly" (`:842-843`). Per pass while open. |

### Related but NOT egui repaint (overlay thread backend)

`crates/overlay/src/service.rs` (standalone thread backend, used only by examples/standalone processes — winit 0.30 allows one `EventLoop` per process, `service.rs:3-7`): repaint is winit `window.request_redraw()` on `LoopEvent::Redraw` (`service.rs:128`) and on egui-winit state repaint (`service.rs:171-172`); loop runs `ControlFlow::Wait` (`service.rs:98`), commands arrive via `EventLoopProxy` (`service.rs:7`). No `request_repaint` here; hidpad-poc has no UI (`hidpad-poc/src/main.rs`, no hits).

No call sites in `tests/` or `src/capture.rs`, `src/gamepad.rs`, `src/tray.rs` — a fired hotkey spawns the capture directly on the listener thread (`src/hotkeys.rs:899-901`), and pad presses land via the mailbox drained on the logic pass (`src/gui/app.rs:905-908`), so neither trigger path repaints.

## Render loop / versions

- Root deps (`Cargo.toml`): `eframe = { version = "0.37.0", default-features = false, features = ["glow", "default_fonts", "x11", "wayland", "links"] }` (`Cargo.toml:26`), `egui = "0.37.0"` (`:33`), path deps `overlay` (`:56`), `toast` (`:57`), `icons` (`:64`), `gamepad` under `[target."cfg(windows)".dependencies]` (`:112-113`). Workspace members incl. `crates/video-capture` (`:13`).
- Locked (`Cargo.lock`): `eframe 0.37.0` (`:1019-1020`), `egui 0.37.0` (`:1054-1055`), `egui-winit 0.37.0` (`:1094-1095`), `winit 0.30.13` (`:5431-5432`), `glow 0.17.0` (`:1690-1691`), `glutin 0.32.3` (`:1702-1703`), `glutin-winit 0.5.0` (`:1727-1728`), `tray-icon 0.26.1` (`:4528-4529`), `global-hotkey 0.9.0` (`:1649-1650`), `xcap 0.9.8` (`:5573-5574`).
- Main loop: `eframe::run_native("virshot", options, create-closure)` in `src/gui/window.rs:182`; `NativeOptions { viewport, ..Default::default() }` (`window.rs:177-180`) — **no `RendererOptions`, no explicit repaint/animation config anywhere** (no `UpdateMode`, no `Frame::request_repaint`). Root viewport `.with_transparent(true)` (`window.rs:156-158`) — eframe derives the alpha-capable GL config from it (comment `:156-157`).
- eframe 0.37 `App` API: `logic(&mut self, ctx, frame)` + `ui(&mut self, ui, frame)`, no `should_exit` — verified against locked source, recorded in `src/gui/app.rs:12-30`. While the window is hidden eframe runs `logic` only (`Context::run_logic`), no UI pass (`app.rs:18-24`, `:44-48`).
- `ToastService::spawn(Arc::new(host.handle()), monitors, Some(cc.egui_ctx.clone()))` in the create closure (`src/gui/window.rs:206-207`) — the wake `Context` is handed to the toast timer thread (`crates/toast/src/service.rs:101-113`); timer thread is plain `std` with `inbox.recv_timeout(deadline)` (`service.rs:157-169`), deadline from `ToastLifecycle::next_deadline` (`crates/toast/src/rules.rs:117-130`): hold-end / 25 ms fade tick / `None` for sticky.
- Overlay thread backend: one `EventLoop<LoopEvent>` with `ControlFlow::Wait` + `EventLoopProxy` (`crates/overlay/src/service.rs:75-98`), built `with_any_thread` for x11/windows (`:81`, `:88`).
- Gamepad pump (no UI repaint): `crates/gamepad/src/gameinput.rs` — `DISPATCH_INTERVAL = 10 ms` + `DISPATCH_QUOTA_MICROSECONDS = 5000` (`:145-147`), `POLL_INTERVAL = 50 ms` state-diff (`:141`), `HEARTBEAT_INTERVAL = 5 s` (`:143`).
- Resident no-GUI park loop: `IDLE_TICK = 1 s` sleep (`src/main.rs:137`, used at `:805`).

## Existing constants

Convention: one module = one file; module-level `const` in SCREAMING_SNAKE_CASE with a doc comment, in the module file itself. No central constants module exists.

- `src/gui/app.rs:104` — `const TRAY_POLL_TICK: Duration = Duration::from_millis(500);` (private, doc comment `:100-103`: keeps idle CPU far under the §5 budget).
- `src/main.rs:137` — `const IDLE_TICK: std::time::Duration = std::time::Duration::from_secs(1);`
- `src/main.rs:149` — `const LOG_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_millis(200);`
- `src/bindings.rs:122` — `const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(250);` (shared ~250 ms trigger debounce gate).
- `crates/toast/src/rules.rs:22` — `pub(crate) const FADE_MS: u64 = 250;`
- `crates/toast/src/rules.rs:26` — `pub(crate) const ERROR_HOLD_MS: u64 = 3000;`
- `crates/toast/src/rules.rs:30` — `pub(crate) const FADE_TICK_MS: u64 = 25;` (repaint cadence while fading; scheduler wakes at this, `rules.rs:125-127`).
- `crates/gamepad/src/gameinput.rs:141-147` — `POLL_INTERVAL` 50 ms, `HEARTBEAT_INTERVAL` 5 s, `DISPATCH_INTERVAL` 10 ms, `DISPATCH_QUOTA_MICROSECONDS` 5000.
- `src/update/mod.rs:40`, `src/update/install.rs:43`, `src/update/notes.rs:24` — `FETCH_TIMEOUT`/`DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10)`.
- Example precedent: `crates/icons/examples/icons_gallery.rs:38` — `const REPAINT_EVERY: Duration = Duration::from_millis(100);`
- Toast default hold lives in config, not a const: `duration_ms` default 2000 (spec §4.3; crate rules take it as a parameter, `rules.rs:44-46`).

## Repaint necessity per surface

Evidence from code only:

- **Settings window (visible)**: repaints on egui input events; nothing in `App::ui` (`src/gui/app.rs:973`) requests repaint unconditionally. The unconditional `request_repaint()` calls in `logic` (`app.rs:449,471,505,510,667,774`) all accompany state transitions that just happened — immediate reaction needed. Continuous repaint is NOT needed here by itself.
- **Settings window (hidden, resident)**: logic-only pass driven by `request_repaint_after(TRAY_POLL_TICK)` (`app.rs:954`) — tray polling cadence, already throttled; plus the update-check deadline min (`app.rs:960`).
- **Toast chips / overlay host**: this is the only continuously-repainting surface. While a chip is alive:
  - toast thread wakes the context only on real changes: Show/Dismiss/Gone (`service.rs:195,199,208`) and fade ticks every 25 ms during `FadeOut` (`service.rs:201-214`, `rules.rs:125-127`); during `Hold` it sleeps until hold-end (`rules.rs:123`) — alpha is constant 1.0 (`rules.rs:107`).
  - BUT `OverlayHost::paint` re-requests every pass regardless of change: `request_repaint_of(viewport)` per live viewport (`host.rs:471`) and `ctx.request_repaint()` + root `RequestPaintWhileHidden` while `has_live` (`host.rs:478-483`). Since each pass re-requests, a steady chip (unchanged content, alpha 1.0) keeps the host loop repainting continuously at full rate; the debug comment at `host.rs:424-431` explicitly assumes "a steady chip would otherwise write one line every repaint".
  - Chip windows are non-interactive (`host.rs:347-355`: mouse passthrough / Windows flag dropped); they have no input-driven repaint need beyond content/alpha changes.
- **About child viewport**: registered + re-requested every logic pass while open (`app.rs:533-571`); content is static changelog + texture — repaint needed only on open/close/icon/notes, not per pass.
- **Update dialog child viewport**: same per-pass registration + `request_repaint_of` (`app.rs:799-844`); content changes only on drained outcomes/notes/button presses.
- **`video-capture` demo**: needs continuous repaint to poll capture-thread acks (comment `examples/capture_demo.rs:1052-1055`) — a genuine continuous-repaint case, but it is an example, not the shipped app.
- **Icons gallery example**: already throttled via `request_repaint_after(REPAINT_EVERY)` (`icons_gallery.rs:123`).
- **hidpad-poc**: no UI, no repaint calls.

## Testing & gates

Relevant test placement for the modules involved (AGENTS.md Testing rules, confirmed on disk):

- `src/gui/app.rs:1090-1091` — sibling tests `#[cfg(test)] #[path = "app.tests.rs"] mod tests;` (file `src/gui/app.tests.rs`).
- `src/hotkeys.rs:969-970` — sibling `src/hotkeys.tests.rs`.
- `crates/toast`: inline `#[cfg(test)]` modules — `rules.rs:138`, `service.rs:299`, `chip.rs:95`, `sink.rs:89`, `image.rs:41`, `color.rs:75`, `placement.rs:61`. No `crates/toast/tests/` directory.
- `crates/overlay`: inline tests — `lib.rs:250`, `monitors.rs:232`, `placement.rs:137`, `host.rs:710` (plus `#[cfg(test)]` helpers at `host.rs:504-528`). No `crates/overlay/tests/` directory.
- `crates/icons`: integration tests `crates/icons/tests/icons.rs`, `crates/icons/tests/error.rs`; inline `lib.rs:52`.
- `crates/video-capture`: sibling `config.tests.rs`, `engine.tests.rs`, `ring.tests.rs`.
- `tests/unit/main.rs` — placeholder only: root package is bin-only (no `lib` target), so no unit modules exist yet (`tests/unit/main.rs:5-9`).
- `tests/e2e/main.rs` declares `mod cli_headless;` → `tests/e2e/cli_headless.rs` (drives the built binary).
- `scripts/check.sh` (read, not run): `./cargo.sh check` → `./cargo.sh test` (with `SKIP_E2E=1` → `cargo test -- --skip 'cli_headless::'`) → `./cargo.sh clippy -- -D warnings` (`check.sh:6-18`). Root package only.
- `scripts/check_full.sh`: `check.sh` + `./cargo.sh fmt` + `./cargo.sh fmt --check` (`check_full.sh:7-9`).
- Crate tests per AGENTS.md: `./cargo.sh test -p overlay -p toast`, or `cd crates/<crate> && ../../cargo.sh test`; workspace at task boundaries. Windows gate: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`.
- Test naming `<unit>_<scenario>_<expectation>`; time is injected in unit tests (AGENTS.md Rust Best Practices; toast rules take `now` as f64 seconds — `rules.rs:51`, `:117`).
- `./scripts/cratedoc.sh <crate>::<item>` available for API verification (`scripts/` listing).

## docs/spec.md excerpts

(spec is Russian; quotes verbatim.)

- §4.3 "Оверлей-уведомление (тост)", `docs/spec.md:363-368`: toast machinery lives in workspace crates — "`crates/toast` (правила тоста на `std`-таймер-потоке: `ToastService`, `ToastHandle`, latest-wins) и `crates/overlay` (бэкенд хост-цикла `OverlayHost` поверх `egui::Context` multi-viewport)".
- §4.3 paint, `docs/spec.md:381-388`: chip is a child egui viewport — transparent, no decorations, always-on-top, passthrough, content-protected; created hidden, painted while hidden (`RequestPaintWhileHidden`), shown only after first paint; "`OverlayHost::update(&ctx)` вызывается из `App::logic`, поэтому плашки появляются и гаснут и при скрытом окне настроек".
- §4.3 animation, `docs/spec.md:401-404`: "**Анимация:** fade-in отсутствует (плашка рисуется с полного непрозрачного первого кадра), fade-out фиксированный ~250 мс; удержание = `duration_ms` (дефолт 2000 мс); `duration_ms = 0` — плашка без авто-скрытия; ошибки держатся ~3000 мс".
- §4.5 tray integration, `docs/spec.md:637-639`: "Интеграция `tray-icon` с event loop: решено (Stage 10) — polling глобальных receivers в UI-цикле eframe (`try_recv` каждый кадр, +repaint каждые 500 мс)".
- §4.6 About, `docs/spec.md:643-647`: About opens a child viewport (`show_viewport_deferred`, id `virshot-about`) on the same eframe cycle — no second `EventLoop` (§6); "Регистрация — каждый logic-pass пока окно живо ... (механика та же, что у тост-чипов §4.3)".
- §2 gamepad pump, `docs/spec.md:185`: "dispatcher `Dispatch(5000µs)` на потоке-помпе + 50 ms опрос кнопок по [STATE-diff]".
- §5 non-functional budget, `docs/spec.md:873`: "**RAM:** < 30 МБ в фоне (без захвата). **CPU:** < 1% при простое." (exe budget < 12 MB at `:861-862`).
- §4.1 debounce, `docs/spec.md:324`: "**Debounce** одного действия ~250 мс".

## Constraints

From the task artifacts and repo conventions:

- Task scope (`.dumbspec/current/2026-11-13_gpu-repaint-throttle/spec.md:4-8`): replace unconditional `request_repaint()` with `request_repaint_after(interval)` where per-frame repaint is not needed; the interval becomes a named constant instead of an inline literal. Out of scope: render-stack changes, overlay/toast window architecture, new dependencies.
- User clarification (`draft.md:7`): **repaint interval = 200 ms**.
- Keep unconditional repaint where immediate reaction is needed (`spec.md:14-15`) — matches the event-driven call sites #1-7, #8-9, #12.
- `unsafe` forbidden in project code (`#![forbid(unsafe_code)]`; overlay: `#![deny(unsafe_code)]` + scoped allow only in `gl_boot.rs`) — AGENTS.md Safety; task spec.md:19.
- No new crates.io dependencies without explicit user approval (AGENTS.md Dependencies; task spec.md:20). All needed APIs already locked: egui/eframe 0.37.
- eframe 0.37 `App::logic`/`App::ui` split; hidden window runs logic-only (`src/gui/app.rs:12-24`) — any interval change must keep tray polling and update-deadline passes alive while hidden (`app.rs:950-961`).
- egui keeps the EARLIEST scheduled repaint — multiple `request_repaint_after` calls are additive, never conflicting (`src/gui/app.rs:955-958` comment).
- Never `request_repaint_of` a viewport without a window — pins `repaint_delay` to 0 and deduplicates later requests (documented in `crates/overlay/src/host.rs:246-250`).
- `RequestPaintWhileHidden` holds for one frame, must be re-sent every pass (`crates/overlay/src/host.rs:465-467`, `:480-483`).
- Thread model: exactly one winit EventLoop per process (winit 0.30 guard, `crates/overlay/src/service.rs:3-7`); toast timer thread is plain `std` `recv_timeout` (AGENTS.md Build & environment); host apps paint overlays via `OverlayHost` on eframe's loop (`src/gui/app.rs:927`).
- Gates: `./scripts/check.sh` per edit batch (check + test + clippy `-D warnings`, no fmt); `./scripts/check_full.sh` at boundaries; crate tests via `./cargo.sh test -p overlay -p toast`; SKIP_E2E filter `cli_headless::` (AGENTS.md, scripts read above). Do not run the full suite in this pass (done — scripts were only read).
- Constants convention: SCREAMING_SNAKE_CASE module-level `const` in the owning module file, doc comment (see `src/gui/app.rs:100-104`, `crates/toast/src/rules.rs:21-30`); one module = one file (AGENTS.md Code structure).
- Comments in repo are English (AGENTS.md Language Policy); spec.md in Russian is product-requirements data.

## Gaps (not determined in this pass)

- Exact egui semantics of `request_repaint_after` (dedup of repeated calls, min-delay floor) were not verified via `./scripts/cratedoc.sh egui::Context::request_repaint_after` in the first pass — repo comments (`src/gui/app.rs:955-958`, `crates/overlay/src/host.rs:246-250`) were the available evidence; AGENTS.md says verify API by building/docs, not memory. **Closed** by the deep pass below (`## Verified egui 0.37 repaint API (deep pass)`): locked sources read + scratch probe executed.
- Actual GPU consumption measurements (GPU usage per pass under glow backend) — no profiler data in repo; `scripts/prof.sh` exists but was not run here.
- Whether egui skips GPU work on a repaint pass with identical content (diff-based redraw) — not determined from locked sources in this pass.
- Whether the settings-window UI pass, while visible, repaints at the display refresh rate whenever any overlay chip is alive — implied by the paint loop (`host.rs:478-483`) but not measured. See the verified section below: the mechanism is confirmed, the *rate* is still unmeasured.

## Verified egui 0.37 repaint API (deep pass)

Method: read the locked sources under `.cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` (`egui-0.37.0`, `eframe-0.37.0`, pinned by `Cargo.lock:1054-1055`) and rendered docs (`./scripts/cratedoc.sh egui::Context --all > .tmp/cratedoc_context.md`; note: rustdoc 1.99.1 emits method items as inline anchors on the struct page, so `cratedoc.sh egui::Context::<method>` reports "item not found" — the struct page is the doc source). Behaviour was then executed in a throwaway scratch crate under `./.tmp/` (see "Probe" below). No repo source was changed and no dependency was added.

### 1. `Context::request_repaint_after(duration)` semantics

Per-viewport state lives in `ViewportRepaintInfo` (`egui-0.37.0/src/context.rs:335-371`), created on demand by `viewports.entry(id).or_default()`. Defaults (`context.rs:373-391`): `repaint_delay = Duration::MAX` ("We haven't scheduled a repaint yet", `:380`), `outstanding = 1` (`:383`), `prev_pass_paint_delay = Duration::MAX` (`:388`).

The internal path is `ContextImpl::request_repaint_after(delay, viewport_id, cause)` (`context.rs:133-174`):

- `delay == Duration::ZERO` sets `outstanding = 1` — the comment "Each request results in two repaints" (`context.rs:141-144`); a non-zero delay leaves `outstanding` alone, so a delayed request produces exactly one repaint (`context.rs:145-150`).
- The repaint observer is called for **every** request, deduplicated or not (`context.rs:160-162`).
- Dedup rule (`context.rs:164-173`): `if delay < viewport.repaint.repaint_delay { repaint_delay = delay; callback(...) }` — a **strict `<`**, so the stored schedule is the **earliest/smallest** delay ever requested, and the backend callback fires only when a request moves the next repaint *earlier* (doc: `.tmp/cratedoc_context.md:464-483` — "only the _smallest_ duration will be considered … 1s and 2s → repaint after 1 second").
- `begin_pass_repaint_logic` (`context.rs:102-127`) resets the schedule at the start of each pass of that viewport: `outstanding == 0` → `repaint_delay = Duration::MAX` (`:113-115`); otherwise `repaint_delay = Duration::ZERO`, `outstanding -= 1`, callback with `delay: Duration::ZERO` (`:118-126`).
- **No minimum-delay floor inside egui** — nothing clamps the `Duration`; `Duration::MAX` is the "nothing scheduled" sentinel. `has_requested_repaint(&ViewportId)` (`context.rs:184-188`) is `outstanding > 0 || repaint_delay < Duration::MAX`.
- egui adds its own requests: `viewport.input.wants_repaint_after()` at pass start (`context.rs:510`) is applied via `request_repaint_after(delay, viewport_id, RepaintCause::new())` (`context.rs:578-580`); it returns `Some(Duration::ZERO)` for pointer/wheel/event input (`egui-0.37.0/src/input_state/mod.rs:676`, `pub(crate)`), widget changes repaint at `context.rs:3236-3238`, in-progress animations repaint at `context.rs:3767`/`context.rs:3793`.
- Timing quirk (doc `.tmp/cratedoc_context.md:485-491`): the duration begins counting at the *next* frame, and eframe turns it into an absolute time — `when = Instant::now() + info.delay` (`eframe-0.37.0/src/native/glow_integration.rs:367-379`, same shape in `wgpu_integration.rs:291-306`). eframe's own floors: invisible/minimized windows are pushed to `now + INVISIBLE_WINDOW_REPAINT_INTERVAL` = 100 ms (`run.rs:27`, applied `run.rs:376-382`), `MISSED_REDRAW_TIMEOUT` = 250 ms (`run.rs:36`), and `sleep_if_invisible_or_minimized` sleeps 10 ms (`winit_integration.rs:25-30`).

### 2. Delayed repaint of a specific child viewport — it exists

Public surface (`egui-0.37.0/src/context.rs`): `request_repaint()` `:2128`, `request_repaint_of(id)` `:2145` (immediate, per viewport), `request_repaint_after(duration)` `:2179` (delegates to `request_repaint_after_for(duration, self.viewport_id())`), `request_repaint_after_secs(seconds: f32)` `:2187`, and **`request_repaint_after_for(duration: Duration, id: ViewportId)` `:2222`** — the per-viewport *delayed* repaint. So yes: a delayed repaint can be aimed at a specific child viewport, via `request_repaint_after_for`.

Nothing else provides it: `ViewportCommand` has **no** repaint variant in 0.37 (`egui-0.37.0/src/viewport.rs:1085-1240`; only `RequestPaintWhileHidden` `:1239` concerns painting while hidden). `ViewportOutput::repaint_delay` (`viewport.rs:1272`, doc `:1297-1303`, combined with `.min()` in `append` `:1308`) is the integration-side channel, but eframe's native integrations **ignore it** (`eframe-0.37.0/src/native/glow_integration.rs:1569`: `repaint_delay: _, // ignored - we listened to the repaint callback instead`). egui-winit exposes no repaint API (it only translates events into egui input). eframe's scheduling path: callback → `UserEvent::RequestRepaint { viewport_id, when, cumulative_pass_nr }` (`winit_integration.rs:77-93`) → `EventResult::RepaintAt(window_id, when)`, and per-window the earliest `Instant` wins (`run.rs:153-161`, `.map_or(repaint_time, |last| (*last).min(repaint_time))`) → `ControlFlow::WaitUntil(next_repaint_time)` (`run.rs:298`).

### 3. Does a root repaint wake child viewports? Only for `Immediate`, not for `Deferred`

`egui-0.37.0/src/viewport.rs:19-33` (module docs) and `ViewportClass` (`:82-109`): a **Deferred** viewport is "repainted independently of the parent viewport"; an **Immediate** one repaints "whenever the parent viewport needs to be repainted, so will the child viewport, and vice versa". Confirmed in eframe: an immediate child cannot repaint itself — it returns `RepaintNext(parent window)` (`glow_integration.rs:654-664`), while a deferred child has its own window and its own schedule.

Therefore a root `Context::request_repaint()` does **not** schedule a deferred child; wake it explicitly with `request_repaint_of(child)` or `request_repaint_after_for(delay, child)`. The only cross-viewport repaint request egui makes is `send_viewport_cmd_to(id, command)` → `request_repaint_of(id)` plus a parent request when `command.requires_parent_repaint()` (`context.rs:4569-4577`), and that predicate is true only for `ViewportCommand::Close` (`viewport.rs:1260-1263`). Child UI runs only on the child's own pass (`eframe-0.37.0/src/native/epi_integration.rs:337-375`); the root pass runs `app.logic` + `app.ui`, and a hidden root without a paint request runs logic-only (`glow_integration.rs:708-755`).

Lifetime caveat: a deferred child survives a pass only if the root pass registers it — `show_viewport_deferred` marks `viewport.used = true` (`context.rs:4628`), and `end_pass` drops children that were never used this pass (`context.rs:3262-3275`) or whose parent is gone (`:3250-3260`). `OverlayHost::paint` re-registers live viewports every pass (`crates/overlay/src/host.rs:360-374`), which is what keeps their schedules alive.

### 4. No public accessor for "seconds until next repaint"

There is **no** public `repaint_since()`, `repaint_wait_time()`, or remaining-delay accessor in egui 0.37, and the only internal source of that number — `InputState::wants_repaint_after()` — is `pub(crate)` (`src/input_state/mod.rs:676`). Public options, in order of usefulness:

- `Context::has_requested_repaint()` `:2241` / `has_requested_repaint_for(&ViewportId)` `:2247` — boolean only.
- `Context::requested_repaint_last_pass()` `:2229` / `requested_repaint_last_pass_for(&ViewportId)` `:2235` — whether a repaint happened in the pass that just ended.
- `Context::cumulative_pass_nr()` / `cumulative_pass_nr_for(id)` `:2095` — pass counter, eframe uses it to drop stale requests (`run.rs:362-369`).
- `Context::repaint_causes()` `:2254` — causes recorded for the current pass, not a schedule.
- `Context::set_repaint_observer(callback)` `:2290` — the **only** public way to see every request including deduped ones, carrying `RequestRepaintInfo { viewport_id, delay, current_cumulative_pass_nr }` (`context.rs:53-67`). Doc (`.tmp/cratedoc_context.md:568-572`): the observer "called for every repaint request … including the ones egui makes itself"; the callback `set_request_repaint_callback` (`:2268`) "only called when a request makes the next repaint of a viewport come sooner". egui's own test proves it: `test_repaint_observer_sees_every_request` (`context.rs:5230`) expects callback `[1s]` vs observer `[1s, 2s]`. Note eframe already installs `set_request_repaint_callback` (`glow_integration.rs:367-379`), and both setters overwrite, so an app cannot reuse the callback slot.
- After a pass: `FullOutput.viewport_output[id].repaint_delay` (`context.rs:3309`, struct `viewport.rs:1272`) — the remaining delay egui reported *for that pass*, post-hoc, per viewport.

### 5. Overlay-host scenario: a pending 200 ms delay never blocks an immediate request

Yes — with child viewports plus a hidden root, `ctx.request_repaint_after(200ms)` followed by an immediate `ctx.request_repaint()` from another thread runs a pass sooner. The dedup is a strict `<` (`context.rs:167`), and `Duration::ZERO < 200ms`, so the immediate request overwrites the schedule and fires the backend callback; eframe then maps it to `RepaintAt(window_id, now)` and keeps the earliest `Instant` per window (`run.rs:153-161`). The pending delay is simply superseded — nothing "waits" for it. Caveats: eframe drops requests whose `cumulative_pass_nr` is stale (`run.rs:366-369`, `:387-390`), drops them entirely when the viewport has no window yet (`window_id_from_viewport_id` → `None` ⇒ `EventResult::Wait`, `run.rs:370-386`, `glow_integration.rs:484-492`), and for an invisible/minimized window raises the wake time to `now + 100ms` (`run.rs:376-382`), so a short delay can wake later than requested. A hidden root without `ViewportCommand::RequestPaintWhileHidden` (`viewport.rs:1239`, re-sent per pass in `crates/overlay/src/host.rs:467`, `:480-483`) runs a logic-only pass (`glow_integration.rs:708-755`).

### 6. Repo comment verification

- `src/gui/app.rs:955-958` ("egui keeps the earliest scheduled repaint; later calls never push it later") — **correct for 0.37**: strict `<` in egui (`context.rs:164-173`) plus earliest-`Instant` per window in eframe (`run.rs:153-161`). The `app.rs:959-961` pattern (`deadline.saturating_duration_since(now).min(TRAY_POLL_TICK)`) is safe: it can shorten, never lengthen. One nuance: while the current pass was triggered by an immediate request, `repaint_delay` is `Duration::ZERO` for the whole pass (`context.rs:118-126`), so a `request_repaint_after(...)` issued *inside* that pass is deduplicated away — the pass still happens, and `end_pass` reports `repaint_delay = ZERO` (`context.rs:3309`), which eframe turns into an immediate next repaint.
- `crates/overlay/src/host.rs:246-250` ("never `request_repaint_of` a viewport without a window: it pins that viewport's `repaint_delay` to ZERO, so every later request is deduplicated away") — **correct in effect**: `request_repaint_of` → `request_repaint_after(ZERO)` sets `repaint_delay = ZERO` and `outstanding = 1` (`context.rs:133-144`), and since no later delay satisfies `delay < ZERO`, the callback never fires again for that entry (`context.rs:167-173`). Nuances worth keeping in the comment's back pocket: (a) eframe would have dropped the event anyway for a window-less viewport (`run.rs:370-386`); (b) the pin only lasts while that viewport entry never runs a pass — `begin_pass_repaint_logic` resets it (`context.rs:113-115`); (c) a phantom entry created only by a repaint request is dropped by `end_pass` retain (`context.rs:3250-3260`, parent not present), after which a fresh entry starts at `Duration::MAX` again. The host's own recovery is exactly what the comment says: the root loop repaint (`ctx.request_repaint()`, `host.rs:251`, `:269`) registers the viewport on that pass (`host.rs:360-374`), and `paint` gates its per-viewport request on the viewport having a real window (`host.rs:471`).

### Probe (throwaway, `./.tmp/`, git-ignored)

Scratch crate `.tmp/repaint-probe/` (own empty `[workspace]` table so it stays out of the repo workspace; deps `egui = "=0.37.0"`, `emath = "=0.37.0"`, `epaint = "=0.37.0"` — all already locked; no repo file touched). `embed_viewports(false)` mirrors eframe on native; `max_passes = 1` and `sync_window_theme = false` keep egui's own bookkeeping from issuing repaints.

```
CARGO_TARGET_DIR=./.cache/cargo/target/tmp-repaint-probe ./cargo.sh run --offline --manifest-path .tmp/repaint-probe/Cargo.toml
```

Output:

```
pass 1 (fresh context): "FFFF"=0ns
pass 2 (idle, nothing requested): "FFFF"=18446744073709551615.999999999s
pass 2 causes: []
requests 200ms then 300ms: callback saw [200ms], observer saw [("FFFF", 200ms), ("FFFF", 300ms)]
has_requested_repaint() = true (no accessor for the remaining delay)
immediate request while 200ms pending: callback saw [0ns], observer saw [("FFFF", 0ns)]
pass after that: "FFFF"=0ns
pass with child (150,150,90 on child; 500 on root): "998C"=90ms, "FFFF"=500ms
callback saw [150ms, 90ms, 500ms]
root-only request: "FFFF"=0ns
pending 400ms on child: has_requested_repaint_for(child) = true, requested_repaint_last_pass_for(child) = false, cumulative_pass_nr_for(child) = 0, repaint_causes(root) = 1
ghost request_repaint_of then two 120ms requests: callback saw [0ns], has_requested_repaint_for(ghost) = true
idle pass after ghost pin: "FFFF"=0ns
all observed requests: [("FFFF", 0ns), ("998C", 150ms), ("998C", 150ms), ("998C", 90ms), ("FFFF", 500ms), ("FFFF", 0ns), ("998C", 400ms), ("1A42", 0ns), ("1A42", 120ms), ("1A42", 120ms)]
```

Reading it: `FFFF` = `ViewportId::ROOT`, `998C` = the deferred chip child, `1A42` = a never-registered ("ghost") viewport. Idle pass ⇒ `Duration::MAX` = nothing scheduled; 200 ms then 300 ms ⇒ callback only `[200ms]` (earliest wins), observer sees both; an immediate request while 200 ms is pending ⇒ callback `[0ns]` and the pass runs; child delays 150/150/90 collapse to 90 ms and the root keeps its own independent 500 ms (Q2 + Q3); a root-only request leaves the child unscheduled and unlisted; the ghost pin shows `request_repaint_of` firing `[0ns]` once and both later 120 ms requests deduped (observer still records them).

### Consequences for the throttle design

- Per-viewport throttle: `ctx.request_repaint_after_for(delay, child_viewport_id)`; never `request_repaint_of` for a viewport without a window (`host.rs:246-251` already encodes that).
- Root-loop throttle while a chip is alive: a chip that only animates on its own schedule can be left on `request_repaint_after_for(...)`, but anything that needs a live root pass (chip registration, `RequestPaintWhileHidden`, tray polling) must keep `ctx.request_repaint()`.
- Delayed requests yield exactly **one** repaint (`outstanding` stays 0), immediate ones **two** (`context.rs:141-144`): continuous animation needs a re-request inside each pass, not a single delayed request.
- eframe ignores `ViewportOutput::repaint_delay` (`glow_integration.rs:1569`) and the 100 ms invisible-window floor can lengthen short delays (`run.rs:376-382`) — a 200 ms interval is above that floor, so the throttle is unaffected, but sub-100 ms delays are silently raised for hidden windows.
