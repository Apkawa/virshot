# 2026-11-12_gui-refactor-update-module: implementation plan (gui decomposition + update/ move)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage ends with the iteration gate `SKIP_E2E=1 ./scripts/check.sh`
(check + test + clippy `-D warnings`, via `./cargo.sh`) and a commit.
This is a behavior-preserving refactor: the existing test suite is the safety net
(TDD red/green does not apply to pure moves — tests travel with their module and stay green).
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan
- [x] draft.md from raw request; refinement confirmed (all `update*.rs` move to `src/update/`)
- [x] research subagent report (gui.rs 3912 lines item map; update trio inventory; conventions)
- [x] spec.md v1; design decision confirmed — variant B: tab paint methods move to tab files,
  `SettingsApp` fields become `pub(super)`
- [x] commit `.dumbspec/current/2026-11-12_gui-refactor-update-module/` to freeze scope (7be932b)

**Criterion:** spec.md is full-format (Created + Changes), plan.md exists; scope committed on branch
`2026-11-12_gui-refactor-update-module`.
**Commit:** `docs(dumbspec): gui decomposition + update-module move — spec and plan`

## Stage 1 — Move auto-update trio into `src/update/`
- [x] `git mv src/update.rs src/update/mod.rs`; `git mv src/update.tests.rs src/update/update.tests.rs`
- [x] `git mv src/update_install.rs src/update/install.rs` (+ `update_install.tests.rs` → `update/install.tests.rs`);
  `git mv src/update_notes.rs src/update/notes.rs` (+ `update_notes.tests.rs` → `update/notes.tests.rs`)
- [x] `src/update/mod.rs`: declare `pub(crate) mod install; pub(crate) mod notes;`; keep test attachment
  pointing at `update.tests.rs`; drop flat `mod update_install; mod update_notes;` from `src/main.rs`
- [x] fix references: `crate::update_install::` → `crate::update::install::` and
  `crate::update_notes::` → `crate::update::notes::` in `src/main.rs`, `src/gui.rs`;
  `update::UpdateInfo` → `super::UpdateInfo` inside `install.rs` (+ its tests)
- [x] gate: `SKIP_E2E=1 ./scripts/check.sh` green

**Criterion:** `src/update/` holds `mod.rs`, `install.rs`, `notes.rs` + sibling `*.tests.rs`;
`rg 'update_install|update_notes' src/` returns no code references; tests from all three files still run.
**Commit:** `refactor(update): move auto-update modules into src/update/` — done: `52a591b`

## Stage 2 — Create `src/gui/`, extract logic-only modules
Execution order adjusted (living journal): submodules are declared while `gui.rs` is still a
file module — Rust 2018+ allows `src/gui.rs` with children in `src/gui/` — so the tree stays
compilable after every step; the `gui.rs` → `gui/mod.rs` rename happens last.
- [x] extract `gui/window.rs` (`INITIAL_WINDOW_*`, `Error`, `host_monitor_rects`, `app_window_icon`, `run`);
  declare `mod window;` in `gui.rs`, re-export `pub(crate) use window::run;` so `gui::run` (main.rs:518) keeps working
- [x] extract `gui/icons.rs` (`IconKind`, `StatusIcon`, `icon_for_status`, glyphs/colors, textures,
  `WARNING_COLOR`/`INFO_COLOR`/`WARNING_ICON_SIZE`/`BIND_BUTTON_MIN_WIDTH`); declare `mod icons;`
  (visibility widened to `pub(crate)` for items used by gui.rs/window.rs — sibling modules cannot see
  private items; note: module name `icons` collides with the path-dep crate `icons`, resolution is correct)
- [x] extract `gui/chrome.rs` (`is_dirty`, `BottomButtons`, `bottom_buttons_enabled`,
  `BOTTOM_BAR_PANEL_ID`, `cancel_draft`, `CloseAction`, `close_decision`); declare `mod chrome;`
  (all items were top-level free items, already `pub(crate)`; no `impl SettingsApp` block needed)
- [x] extract `gui/binding.rs` (`SUPPORTED_KEY_NAMES`, `MOD_NAMES`, key/pad capture helpers,
  duplicate/save gate, registration retry, capture state-machine); declare `mod binding;`
  (35 items; `pub(crate) mod binding` — hotkeys.tests.rs is a sibling of gui, needs the path;
  `src/hotkeys.tests.rs` fixed to `crate::gui::binding::binding_label`; 63 tests → `binding.tests.rs`)
- [x] `git mv src/gui.rs src/gui/mod.rs`; keep overview module doc there (design-doc notes
  §4.5/§4.7 move with app/update in Stage 3–4); submodule declarations stay
- [x] split `gui.tests.rs`: icons/chrome/binding test fns moved with their modules into
  `gui/icons.tests.rs` (6), `gui/chrome.tests.rs` (9), `gui/binding.tests.rs` (63) via `#[path]`
  attachments; `gui.tests.rs` → `gui/gui.tests.rs` (63 tests kept, locale fixture stays there);
  `src/hotkeys.tests.rs` → `crate::gui::binding::binding_label`
- [x] gate green (each extraction step ran the gate; 594 passed, clippy clean)

**Criterion:** `src/gui/` exists; window/icons/chrome/binding files hold their items with test siblings;
`gui.rs` (mod.rs) still compiles as app+tabs+update remainder; clippy clean.
**Commit:** `refactor(gui): extract window/icons/chrome/binding modules from gui.rs`

## Stage 3 — Tab modules: `general.rs`, `hotkeys.rs`, `overlay.rs`
- [x] make `SettingsApp` fields `pub(super)`; cross-file methods `pub(super)` where needed
  (visibility stays inside `gui/` — no crate-wide `pub`) — shipped as `pub(crate)` instead: repo has
  zero `pub(super)` and tests outside `gui/` (src/i18n.tests.rs, src/hotkeys.tests.rs) consume
  `gui::binding`/`gui::overlay`; bare `pub` still never appears in src/gui
- [x] extract `gui/general.rs`: `impl SettingsApp { general_tab, browse_save_path }` + helpers
  (`browse_start_dir`, placeholder accordion, `format_label`, language options/`locale_code_of`,
  update-interval/channel/auto-install controls, `update_check_selected_text`)
- [x] extract `gui/hotkeys.rs`: `impl SettingsApp { hotkeys_tab, status_icon_for, registration_failure,
  has_failed_registration, paint_warning_slot, set_keyboard_capture, start_pad_capture, cancel_pad_capture,
  cancel_active_capture, drain_pad_capture, apply_capture_transition, handle_capture_press,
  commit_probe_press, handle_pad_capture_press }` + `HOTKEY_ROWS`/`row_label` (16 methods + 6 free items;
  `unbound_save_tooltip`/`duplicate_save_tooltip` went to `binding.rs`; 4 tests → `hotkeys.tests.rs`,
  2 tooltip tests → `binding.tests.rs`)
- [x] extract `gui/overlay.rs`: `impl SettingsApp { overlay_tab, publish_overlay_test_chip }` +
  overlay-test chip helpers (`OVERLAY_TEST_FILENAME`, `overlay_test_*`, `test_overlay_toast`, `position_label`);
  fix `src/i18n.tests.rs:114-115` → `crate::gui::overlay::...` (also: `use ::overlay::…` in mod.rs —
  the child module shadows the extern `overlay` crate; unavoidable, gate green)
- [x] move matching test fns into `gui/general.tests.rs` (11), `gui/hotkeys.tests.rs` (4),
  `gui/overlay.tests.rs` (5); locale fixtures duplicated per file where used
- [x] gate green (each step ran `SKIP_E2E=1 ./scripts/check.sh`; 594 passed, clippy clean)

**Stage-3 acceptance finding (2026-11-12):** splitting `gui.tests.rs` created 10 per-file
`LOCALE_LOCK` mutexes; `rust_i18n` 4.2.4 locale is process-wide (`CURRENT_LOCALE` + global
`LOCALE_VERSION`, per-thread cache keyed on it), so locale-loop tests in one file race with
en-pinned tests in another → parallel suite fails ~4/6 runs (serialized is clean; HEAD 3af9839
was clean because one file held one lock). User-approved fix: single crate-level
`#[cfg(test)] pub(crate) static LOCALE_LOCK` in `src/main.rs`; every locale-touching test file
keeps its own `locale_guard()` helper but locks the shared static. Added as a Stage-3 checkbox below.
- [x] shared locale lock: `crate::LOCALE_LOCK` replaces per-file `static LOCALE_LOCK` in
  `gui/{gui→app,binding,general,hotkeys,icons,overlay}.tests.rs`, `i18n.tests.rs`, `capture.tests.rs`,
  `tray.tests.rs`, `about.tests.rs`; parallel suite stable (6 full runs green, later 8 more) —
  landed inside 4077b71

**Criterion:** tab paint methods live in their own files; app remainder in mod.rs (or app.rs) only
holds state/event-loop; all locale-dependent tests still pass; no crate-wide `pub` added.
**Commit:** `refactor(gui): split settings tabs into general/hotkeys/overlay modules`

## Stage 4 — Update UI module `gui/update.rs`, finish the split
- [x] extract `gui/update.rs`: statuses (`UpdateStatus`, `UpdateDialogButton/Outcome`),
  `UpdateDialogWindow`, `UpdateDialogView`, `paint_update_dialog`, chips, forced-check/schedule/
  install decision fns, `impl SettingsApp { open_update_dialog, dismiss_update_dialog, start_install,
  publish_update_chip }`; §4.7 doc notes here (7 types + 23 fns + 4 methods; mod.rs 1773→1155;
  `mod update;` shadows `crate::update` → backend paths fully qualified `crate::update::…` in mod.rs)
- [x] move update UI tests (gui.tests.rs ~2079–2850) into `gui/update.tests.rs` (38 tests + 2 fixtures;
  gui.tests.rs down to 3 tests)
- [x] move remaining app state/event-loop (`SettingsApp` struct, `new`, `apply`, `Tab`,
  `TRAY_POLL_TICK`, `impl eframe::App`) into `gui/app.rs`; `mod.rs` keeps overview doc + declarations
  (+ re-exports `app::SettingsApp`, `window::run`; app.rs 1070 lines, mod.rs 104; 20 fields + 3 methods
  widened to pub(crate) — items in app.rs are invisible to siblings)
- [x] move remaining tests into `gui/app.tests.rs` (3 tests + locale/binding fixtures copied);
  delete leftover `gui.tests.rs`
- [x] gate green (check.sh 594/0/2, clippy --all-targets clean)

**Criterion:** no `src/gui.rs` — only `src/gui/*.rs`; every file has exactly one responsibility;
`gui::run` unchanged for main.rs; full unit suite green.
**Commit:** `refactor(gui): split update UI and app loop into gui/update.rs and gui/app.rs`

## Stage 5 — Final gates (formatting + workspace)
- [x] comment hygiene: stale prose citations `src/gui.rs:<line>` / `src/gui.tests.rs` in
  `crates/toast/src/{chip,color}.rs`, `crates/toast/examples/toast_debug.rs`, `crates/overlay/*`,
  `src/config.rs`, `docs/spec.md` → re-point to the new `src/gui/<module>.rs` paths, drop stale line numbers
  (22 files, comment-only; `rg 'src/gui\.rs|gui\.rs:' src crates docs tests` → 0 hits)
- [x] qualify crate-`icons` paths inside `src/gui/icons.rs` (`::icons::rgba` style) to avoid
  name ambiguity with the new `gui::icons` module (audit: only mod.rs prose needed `::icons::`; child-module
  paths in app.rs/icons.rs/general.rs/hotkeys.rs resolve correctly — verified, left alone)
- [x] full e2e run: `./scripts/check.sh` (without SKIP flag) at stage boundary (unit 594/0/2,
  e2e 9 passed / 1 ignored, exit 0)
- [x] `./scripts/check_full.sh` (fmt + fmt --check) — formatting drift goes to `cargo fmt` (exit 0)
- [x] `./cargo.sh clippy --workspace --all-targets -- -D warnings` (exit 0, overlay/toast/icons/gamepad members clean)
- [x] commit any leftover fixes (3716efb docs: re-point stale citations)

**Criterion:** formatting-aware gate and workspace clippy pass on the branch.
**Commit:** N/A (no new changes expected; fixes committed if any)

## Closure (2026-11-12, final acceptance)
- Final independent acceptance over `7be932b..3716efb`: PASS on scope items 1–7 — update trio moved
  to `src/update/` (rename similarity ~99%, call sites `update::install::…`/`update::notes::…`),
  `src/gui.rs` replaced by 10 files with 9 sibling test files, gui/update test inventory identical
  (143 + 109 names), zero behavior delta (token-multiset diff, restart contract byte-identical,
  Cargo.toml/lock untouched), gates green incl. full e2e (9/0/1), workspace clippy, Windows-gnu
  compile gate.
- Documented deviations: `pub(crate)` instead of planned `pub(super)`; overlay-test items in
  `gui/overlay.rs` (spec responsibility table, spec §73 wording was inconsistent); extra
  `pub(crate) use app::SettingsApp;` re-export; crate-wide `LOCALE_LOCK` consolidation (test-only,
  outside spec Scope — docs/spec.md §locale wording updated accordingly).
- Archive to `.dumbspec/archive/` NOT done: awaiting explicit user confirmation.
