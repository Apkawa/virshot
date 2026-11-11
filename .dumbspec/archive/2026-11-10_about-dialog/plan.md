# 2026-11-10_about-dialog: implementation plan (About dialog from tray)

Spec — [spec.md](./spec.md); research — [research.md](./research.md).

**Format:** each stage = TDD (red test → green implementation → refactor) + a commit at the end.
This file is a **living progress journal** — update statuses as work proceeds.

Legend: `[ ]` not started · `[~]` in progress · `[x]` done.

---

## Stage 0 — Research + spec + plan
- [x] Raw request captured into `draft.md`
- [x] Branch `2026-11-10_about-dialog` created from clean `master`
- [x] Initial spec skeleton written from the draft; refinement gate passed (user: no edits, proceed)
- [x] Research R1 — tray + GUI/window wiring (`src/tray.rs`, `src/gui.rs`, `OverlayHost`, locked versions)
- [x] Research R2 — version metadata, ZeroVer, CHANGELOG/git-cliff, release packaging, dependency availability
- [x] Spec v1 written, open questions resolved with the user, spec updated to v2
- [x] This plan written

**Criterion:** `draft.md`, `research.md`, `spec.md` (v2 with Created + Changes), `plan.md` exist; every scope decision has an explicit user answer recorded in the spec Changes list.
**Commit:** `docs(dumbspec): add About dialog spec and plan`

## Stage 1 — Verify shadow-rs / egui_commonmark APIs and pick feature sets
- [x] Fetch docs/source of `shadow-rs` 2.0.0 with `default-features = false`: which constants are generated, which features are required for version + build timestamp, whether the timestamp is UTC or local
- [x] Fetch docs/source of `egui_commonmark` 0.26.0: minimal feature set that renders plain markdown with no image loading / regex, widget API (`CommonMark`, `install`, `mount`)
- [x] Decide how the Moscow-offset (`+03:00`) build date is produced and how `build.rs` re-runs on every build (fresh date), and record the decision
- [x] Append all verified facts to `research.md` (R3), update `spec.md` (Changes entry) if any decision moves
- [x] Add both deps to `Cargo.toml` with the chosen features, resolve `Cargo.lock` (`./cargo.sh check`)

**Criterion:** `./cargo.sh check` green with both deps present; the exact generated constant names, the minimal `egui_commonmark` feature list, and the date mechanism are written in `research.md` (no "unverified" left for these items); `Cargo.lock` gains no `git2`/`libgit2-sys`/`fancy-regex`/`image`, and `egui` stays at 0.37.0.
**Commit:** `build(deps): add shadow-rs and egui_commonmark for the About dialog`

## Stage 2 — Build metadata + `--version` output
- [x] Red: `src/about.tests.rs` — formatting of the version line (version + short sha + build date), injected values, deterministic
- [x] Red: extend `tests/e2e/cli_headless.rs` version contract to require sha and build date
- [x] Green: `build.rs` in the root package — `shadow-rs` metadata (`BuildPattern::RealTime`) + `cargo:rustc-env=VIRSHOT_BUILD_TIME` (Moscow `+03:00`, computed on `std`; failures degrade to the `BUILD_TIME_3339` fallback, never a panic), fresh build date each build
- [x] Green: wire the metadata into the CLI so `virshot --version` prints version + sha + date
- [x] Refactor: drop redundant clones/env reads, keep `#![forbid(unsafe_code)]` intact
- [x] Gate: `./scripts/check.sh`

**Criterion:** `./cargo.sh run -- --version` prints version, short commit sha, and ISO date with `+03:00`; unit test in `src/about.tests.rs` passes deterministically; e2e version test passes (`./cargo.sh test --test e2e`, or `SKIP_E2E=1` compile-only at iteration time).
**Commit:** `feat(about): About dialog with shadow-rs build metadata` (`9044bd5`)

## Stage 3 — Tray menu item + About window (child viewport)
- [x] Red: `src/tray.tests.rs` — menu has 4 ids incl. `virshot-about`, `action_for_menu_id` maps it to `TrayAction::About`, labels tuple extended
- [x] Red: `src/i18n.tests.rs`-covered locale completeness — new `tray.about` + About text keys in all six locales
- [x] Green: `src/tray.rs` — 4th `MenuItem`, id const, action mapping, `menu_labels()` extended, `refresh_menu` rebuild
- [x] Green: `src/about.rs` — About state + `show_viewport_deferred` registration each logic pass while alive, decorated window, close handling that stops re-registration
- [x] Green: `src/gui.rs` — handle `TrayAction::About` in `logic` (open + `request_repaint()`), works while settings window is hidden; Exit does not hang on an open About window
- [x] Green: `src/main.rs` — `park_without_gui` degrades `About` to WARN (no egui context)
- [x] Refactor: keep `tray.rs` = lifecycle + event mapping ONLY, `about.rs` = window content only
- [x] Gate: `./scripts/check.sh` + `./cargo.sh test -p overlay` if touched

**Criterion:** right-click tray menu shows the localized About item; selecting it opens a decorated About OS window from the single eframe loop, including when the settings window is hidden; closing About and exiting via tray both terminate cleanly. Verified headless where possible, UI-verified if a display is available.
**Verified here (headless):** menu order/ids/labels pinned by `src/tray.tests.rs` in all six locales, the viewport registration shape and the Exit-vs-About behaviour are recorded in R6; no desktop tray UI was driven in this sandbox (no display-bound tray host), so the on-screen check stays for the Windows acceptance run.
**Commit:** `feat(about): wire About dialog into tray menu and version contract` (`bf7988e`)

## Stage 4 — Embed and render CHANGELOG.md
- [x] Red: `src/about.tests.rs` — embedded changelog is non-empty and starts at a git-cliff heading
- [x] Green: `include_str!("../CHANGELOG.md")` in `src/about.rs` (path relative to the module file)
- [x] Green: render the whole changelog with `egui_commonmark` in a scrollable area of the About window
- [x] Gate: `./scripts/check.sh`; measure release size and report it against the `< 10 МБ` budget (`docs/spec.md:551`)

**Criterion:** the About window renders the embedded changelog markdown (headings and list items visible) with no network/image fetching; the release build size delta is recorded in `research.md`.
**Size delta:** 15,124,864 → 15,716,256 bytes (+591,392) — recorded in R6; the budget was already exceeded before this task.
**Commit:** `feat(about): wire About dialog into tray menu and version contract` (`bf7988e`)

## Stage 5 — Spec/docs update and final gates
- [x] Update `docs/spec.md` §4.5 (tray menu gains the About item) and add an About section; update §8 CLI with the new `--version` output
- [x] Windows compile gate: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu`
- [x] Full gate: `./scripts/check_full.sh` (check + test + clippy `-D warnings` + `cargo fmt --check`)
- [x] Append a `spec.md` Changes entry if docs forced any scope tweak

**Criterion:** `docs/spec.md` documents the About dialog and the extended `--version`; formatting-aware gate and the Windows target compile pass.
**Commit:** `feat(about): wire About dialog into tray menu and version contract` (`bf7988e`)

## Stage 6 — Enable clickable links (approved dep)
- [x] Red: `src/about.tests.rs` — pin the repo URL constant to `https://github.com/Apkawa/virshot` (fails before the constant exists)
- [x] Green: `src/about.rs` — `pub(crate) const REPO_URL` with provenance comment; `./cargo.sh test --bins` green (417 passed, 0 failed)
- [x] Green: `Cargo.toml` — eframe features gain `links`; `Cargo.lock` gains `webbrowser` 1.2.6 and nothing else (done in worktree: egui-winit `links → webbrowser` in the feature tree, lock 542 → 543 package names; lands in the Stage 7 commit because Linux clippy needed `REPO_URL` wired)
- [x] Refactor: confirm no extra transitive packages beyond `webbrowser`; keep other feature sets untouched (verified: package-name list diff = only `webbrowser`, 542 → 543; transitive deps already in lock)
- [x] Gate: `./scripts/check.sh` + Windows compile gate `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` (both green after Stage 7 wired `REPO_URL`; Windows-gnu compiles with webbrowser)

**Criterion:** `./cargo.sh tree -e features -i egui-winit` shows `links` enabled; the `Cargo.lock` diff adds only `webbrowser`; both compile gates green (webbrowser builds for Windows-gnu too).
**Commit:** `feat(about): opaque About background, branded header, GitHub link, build line` (`1f77393`, shipped together with Stage 7 — Linux clippy required REPO_URL wired)

## Stage 7 — About layout redesign (opaque background, margins, header, link, build line)
- [x] Red: `src/about.tests.rs` — build line `<sha> <date> · debug|release` from injected inputs (deterministic); title line `ViRShot <version>`; the old `metadata_lines` tests replaced (red: 7 × E0425 — `build_line` ×5, `app_title` ×2)
- [x] Red: `src/i18n.tests.rs` — new keys (`about.app_title`, `about.repo_link`, debug/release marker keys) present in all six locales; key count updated (red: 5 failures — 3 count assertions 85≠84, missing `about.app_title`, dead `about.version` still present; net math −5 dead keys +4 new = 84)
- [x] Green: `locales/app.yml` — −5 dead labels (`about.version/commit/branch/build_debug/build_release`), +4 keys (`about.app_title`, `about.repo_link`, `about.marker_debug`, `about.marker_release`) × 6 locales → 84 keys; `about.app_title` added to `NOTATION_KEYS` in `src/i18n.tests.rs` (brand notation is verbatim by design, R8)
- [x] Green: `src/about.rs` — opaque `egui::CentralPanel` with `Frame::central_panel(style).inner_margin(Margin::symmetric(16, 8))` covering the whole window; header row (icon texture + `RichText::size(22).strong()`); GitHub `ui.hyperlink_to`; build line below the links (`weak_text_color()`, R8); `metadata_lines` removed
- [x] Green: `src/gui.rs` — icon texture owned by `SettingsApp` (`icons::rgba(APP_ICON_PNG)` → `ColorImage::from_rgba_unmultiplied` → `ctx.load_texture`, loaded once on first About open), `Arc<TextureHandle>` passed into the viewport closure; `App::clear_color` stays transparent (toast/overlay depend on it)
- [x] Refactor: no per-pass texture rebuild, no clones beyond the Arc handle; `about.rs` keeps the single window-content responsibility
- [x] Gate: `./scripts/check.sh` (`SKIP_E2E=1`, e2e target compiled and filtered) — 421 passed, 0 failed; `clippy -D warnings` zero warnings; Windows `test --no-run` green

**Criterion:** the About window paints an opaque background covering the full window rect with left/right padding; header shows the app icon + `ViRShot <version>`; the GitHub link is clickable and hands off to the OS browser (no HTTP from our process); build line reads `<sha> <date> · release|debug`; version/branch metadata lines are gone; unit tests stay deterministic. On-screen check if a display is available.
**Commit:** `feat(about): opaque About background, branded header, GitHub link, build line` (`1f77393`)

## Stage 8 — Docs amendment + final gates + size
- [x] Update `docs/spec.md` §4.6 (GitHub link is in scope; links open via the OS browser, the app itself performs no network request) and the §5 security bullet
- [x] Windows compile gate: `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` — green, 3 executables (`./.tmp/stage8_win.log`)
- [x] Full gate: `./scripts/check_full.sh` (`SKIP_E2E=1`) — green, `cargo fmt --check` clean (`./.tmp/stage8_full.log`)
- [x] Re-measure release size and record the delta in `research.md` — 15,799,608 bytes, +83,352 (+0.53%) vs the v2 baseline 15,716,256
- [x] Append a `spec.md` Changes entry if the docs forced any scope tweak — appended (the docs amendment confirmed the v3 scope; no task-scope change beyond the §5 wording)

**Criterion:** `docs/spec.md` matches the shipped About (links + build line); formatting-aware gate and Windows compile gate pass; the size delta is recorded.
**Commit:** `docs(spec): align the product spec with the shipped About dialog` (`ee125aa`)

## Journal (execution of the v3 refinements, 2026-11-10)
- Stages 6-7 shipped in one commit (`1f77393`): the `links` dependency and the layout that uses `REPO_URL` cannot be split without breaking `clippy -D warnings`, so Stage 6's gate is recorded against the combined commit. Deviation from R7: faint text is `Visuals::weak_text_color()` (no `faint_text_color` field in egui 0.37) — R8.
- Stage 8 shipped (`ee125aa`): docs amendment + final gates. Release size 15,799,608 bytes (+83,352 vs v2 baseline).
- On-screen acceptance (Stage 8 criterion "if a display is available") NOT run: this host is headless. Open questions 6 (compositor behaviour of the opaque fill) and 7 (link hand-off on Windows) stay open until the Windows acceptance run; task stays in `.dumbspec/current/` per the user's choice.

## Stage 9 — Window chrome: no minimize/maximize buttons, resize stays
- [x] Red: `src/about.tests.rs` — chrome policy contract test `window_chrome_keeps_title_close_and_resize_but_drops_minimize_maximize` (red: E0425 `cannot find function window_chrome`, ./.tmp/stage9-red.log)
- [x] Green: `src/about.rs` — `pub(crate) struct WindowChrome` (`decorations`, `resizable`, `close_button`, `minimize_button`, `maximize_button`) + `pub(crate) fn window_chrome()` (:21-48) returning true/true/true/false/false, with the platform note (Windows/macOS honor it, X11/Wayland inert — R9)
- [x] Green: `src/gui.rs` — the About `ViewportBuilder` is built **from** `window_chrome()` (:2249-2266); `.with_title(...)` + `.with_inner_size([480.0, 360.0])` kept; constant per viewport id so `ViewportBuilder::patch()` never recreates the OS window (R9)
- [x] Refactor: builder flags stay in `gui.rs` (viewport plumbing), the *policy* stays in `about.rs`; no runtime `EnableButtons` command, no `unsafe`, `App::clear_color` and other viewports untouched
- [x] Gate: `SKIP_E2E=1 ./scripts/check.sh` green (422 passed, 1 ignored, clippy `-D warnings` clean — ./.tmp/stage9-check.log); Windows gate `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green (./.tmp/stage9-win-no-run.log); formatting-aware gate green before the commit

**Criterion:** the About window keeps its title bar, close button and OS resize border but shows no minimize/maximize buttons (verified on Windows; inert by design on X11/Wayland); unit tests green; both gates green.
**Commit:** 2ec03cf

## Stage 10 — Closing the About window (OS close; tray re-select focuses)
- [x] Red: `src/about.tests.rs` — 5 lifecycle tests (red: E0433 `cannot find type AboutWindow` ×6, ./.tmp/stage10-red.log): tray select while closed opens; tray select while open stays open (focus path, user decision 2026-11-11); observed close stops registration; no double close; re-open installs a fresh flag
- [x] Green: `src/about.rs` — `pub(crate) struct AboutWindow { open, close_flag: Arc<AtomicBool> }` (:26-100) with `new()`, `open()`, `select_from_tray()`, `observe_close()`, `wants_registration()`, `close_flag()` (no separate `close()` — `observe_close()` owns the transition, clippy would flag an unused one)
- [x] Green: `src/gui.rs` — dead root-side `raw.viewports…close_requested()` read removed (:2234-2295); `observe_close()` runs in the logic pass, registration gated on `wants_registration()` ⇒ egui GC drops the viewport and eframe drops `Arc<Window>` (window destroyed — R10)
- [x] Green: `src/gui.rs` — tray `About` (:2205-2230): closed ⇒ open (lazy icon + repaint, as before); already open ⇒ `ctx.send_viewport_cmd_to(about_id, ViewportCommand::Focus)` raises it, registration untouched; no `Close`/`CancelClose`/`Visible(false)`; `Exit` path untouched
- [x] Green: `src/about.rs` — `paint(...)` takes `&AtomicBool` and records `ui.input(|i| i.viewport().close_requested())` inside the CHILD pass (:217-228, set-only, `Ordering::Relaxed` — same main thread)
- [x] Refactor: one shared flag, `about_open: bool` replaced by `about_window: AboutWindow` (:1342, ctor :1391); O(1) Arc clone handed to the closure; teardown left to eframe GC, not hand-managed; Stage 9 chrome builder, title/inner size and `App::clear_color` untouched
- [x] Gate: `SKIP_E2E=1 ./scripts/check.sh` green (427 passed, 1 ignored, clippy `-D warnings` clean — ./.tmp/stage10-check.log); Windows gate `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green (./.tmp/stage10-win-no-run.log)

**Criterion:** closing the About window by its OS button removes it (registration stops → window destroyed), re-selecting the tray item raises the open window instead of closing it, re-opening afterwards works with the same viewport id, and tray `Exit` still shuts down cleanly. Runtime observation on this host only if Xvfb can reach the tray; otherwise deferred to the Windows acceptance run.
**Commit:** 91b3b8c

## Stage 11 — Runtime verification attempt, docs, final gates
- [x] Runtime attempt (`./.tmp/stage11-about-verify.sh`, `xvfb-run` + `dbus-run-session`, About opened only via the tray path: dbusmenu `GetLayout` → item `About` → `Event(id,"clicked")`): WITHOUT WM (`./.tmp/stage11-about-nwm.log`) the window appears (`4194313`, name `About virshot`, `IsViewable`, 480×360 shot with 454 unique colors) and a re-select keeps exactly one window with the SAME id, process alive; WITH openbox (`./.tmp/stage11-about-wm.log`) `ViewportCommand::Focus` really raises it (`getactivewindow` == About id), a WM close-button click logs `Received WindowEvent::CloseRequested for viewport Some("F99B")`, the window is gone, settings stays mapped, pid stays resident, and the tray re-opens a fresh window (`6291478`)
- [x] Honest SKIP: no close provable WITHOUT a WM here (`wmctrl -ic` and ctypes `XSendEvent` deliver but winit 0.30.13 does not select `EventMask::CLIENT_MESSAGE` — `x11/window.rs:261-272`; `xdotool 3.2` has no `windowsend`, `windowkill` kills the client). Stage 9 button visibility is not judgeable on X11 (winit `set_enabled_buttons` is a documented no-op; openbox draws its own buttons) — both deferred to the Windows acceptance run
- [x] Docs: `docs/spec.md` §4.6 (:556-573) now states the OS chrome (resize + close on, minimize/maximize off, X11/Wayland limitation, Windows acceptance deferred), the close mechanics (child paint-pass flag → unregister → `Arc<Window>` drop, app stays resident, no `Close`/`CancelClose`/`Visible(false)` for children) and the tray re-select raise; layout wording and the no-network promise untouched
- [x] Gate: `SKIP_E2E=1 ./scripts/check_full.sh` green (427 tests, clippy clean, `cargo fmt` rewrote nothing — ./.tmp/stage11-check-full.log)
- [x] Size: `./cargo.sh build --release` → 15,802,168 bytes (+2,560 vs the R8 baseline 15,799,608; R11 in research.md)

**Criterion:** docs match the shipped window; formatting gate green; the runtime claim is stated as verified or honestly deferred.
**Commit:** b6cdbe3

## Journal (execution of the v4 refinements, 2026-11-11)
- Stage 9 shipped (`2ec03cf`): chrome policy `about::window_chrome()` feeding the About `ViewportBuilder` at creation; builder stays constant for id `virshot-about` so `ViewportBuilder::patch()` never recreates the OS window (R9). Deviation: none; the criterion (no min/max buttons) is Windows/macOS-only and cannot be observed on this X11 host — recorded as open question 11.
- Stage 10 shipped (`91b3b8c`): the dead root-side `raw.viewports…close_requested()` read removed; `paint` records the close inside the child pass into `Arc<AtomicBool>`, the next logic pass stops registration and eframe drops the `Arc<Window>` (R10); tray re-select sends `ViewportCommand::Focus` per the user decision instead of toggling closed. Deviation: no separate `close()` method — `observe_close()` owns the transition (an unused method would fail `clippy -D warnings`).
- Stage 11 shipped (`b6cdbe3`): headless runtime verification through the product tray path (dbusmenu `GetLayout` → `Event(id,"clicked")`) — window appears, re-select keeps one window with the same id, `Focus` raises it under openbox, the WM close button logs `WindowEvent::CloseRequested for viewport Some("F99B")` and destroys the window while the process stays resident, and the tray re-opens a fresh window. Honest skips: close is unprovable without a WM (winit does not select `EventMask::CLIENT_MESSAGE`), and button visibility is not judgeable on X11. Docs §4.6 amended; `check_full.sh` green; release 15,802,168 bytes (+2,560, R11).
- Still open and deferred to the Windows acceptance run: question 6 (compositor behaviour of the opaque fill), 7 (link hand-off on Windows), 10 (OS close on Windows), 11 (min/max button visibility). On-screen acceptance remains un-run on this headless host; the task stays in `.dumbspec/current/` per the user's choice — no archiving.

---

## Stage 12 — Version `-dev` suffix and build line without the debug/release marker
- [x] Red: `src/about.tests.rs` — pure helper contract `version_with_dev(version, tag)`: `tag == "v" + version` → bare version; empty tag (local build) → `<version>-dev`; a non-release tag (`nightly`) → `<version>-dev`; plus `build_line` asserting `<sha> <date>` with no marker and `app_title` rendering the dev version
- [x] Green: `src/about.rs` — `version_with_dev` (: pure fn) used by `version_from_build()` with `build::TAG` (R12: shadow-rs fills `TAG` from `git tag -l --contains HEAD`, CI fallback from `GITHUB_REF`), the About header passes the same dev version to `app_title(...)`; `build_line` loses the channel parameter and the marker lookup
- [x] Green: `locales/app.yml` — drop `about.marker_debug` and `about.marker_release` (84 → 82 keys); `src/i18n.tests.rs` key count and dropped-key assertions updated; `NOTATION_KEYS` unchanged
- [x] Refactor: `build.rs`, CI workflows and `scripts/release.sh` untouched (detection is runtime-only); `build::BUILD_RUST_CHANNEL` stops being consumed anywhere
- [x] Gate: `SKIP_E2E=1 ./scripts/check.sh` green + `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` green

**Criterion:** on this (untagged) HEAD a local build prints `virshot 0.1.0-dev (<sha>) built <ISO +03:00>` and the About header reads `ViRShot 0.1.0-dev`; a build whose HEAD carries `v<version>` prints the bare version; the build line is `<sha> <date>`; catalog has 82 keys; both gates green.
**Commit:** 411254e feat(about): mark untagged builds as -dev and drop the debug/release marker

## Stage 13 — Docs, runtime spot-check, final gates and journal
- [x] Runtime spot-check: `./cargo.sh build` + `./cargo.sh run -- --version` prints the `-dev` suffix; headless About screenshot (Xvfb, tray path as in Stage 11) shows `ViRShot 0.1.0-dev` and `<sha> <date>` with no marker
- [x] Docs: `docs/spec.md` §4.6 — build line without the marker, version shows `-dev` for untagged builds; §8 — `--version` contract gains the `-dev` suffix (only the `<ver>` token changes)
- [x] Gate: `SKIP_E2E=1 ./scripts/check_full.sh` green; release size re-measured and recorded in research.md (R13)
- [x] Journal: spec `Changes` + plan journal entry with the stage commits

**Criterion:** docs match shipped output; full formatting gate green; the `-dev` suffix observed in CLI output and in the About screenshot.
**Commit:** (journal commit below)


## Journal (execution of the v5 refinements, 2026-11-11)

- Stage 12 (`411254e`): `version_with_dev(version, tag)` added in `src/about.rs` — bare version iff `tag == "v" + version`, otherwise `<version>-dev`; wired into `version_from_build()` (CLI) and the About header `app_title(...)`. `build_line(short_sha, build_time)` now renders `<sha> <date>`; the localized marker lookup and `build::BUILD_RUST_CHANNEL` are gone. Catalog keys `about.marker_debug`/`about.marker_release` deleted from `locales/app.yml` (84 -> 82 keys); `src/i18n.tests.rs` pins updated (counts 82, redesigned 5, dropped 7). TDD: red = 4 compile errors (`version_with_dev` missing, `build_line` arity) then 4 catalog failures; green = 430 unit tests, clippy clean. Gates: `SKIP_E2E=1 ./scripts/check.sh` exit 0, `./cargo.sh test --no-run --target x86_64-pc-windows-gnu` exit 0 (./.tmp/stage12-*.log). `build.rs`, CI and `scripts/release.sh` untouched.
- Stage 13 (`<journal commit>`): CLI spot-check on untagged HEAD prints `virshot 0.1.0-dev (411254e) built 2026-11-11T12:21:35+03:00` (./.tmp/stage13-cli-version.txt). `docs/spec.md` §4.6 — build line now `<short sha> <date>` with the marker removed, plus a bullet for the `-dev` suffix; §8 — `--version` contract states the `-dev` suffix on the `<version>` token only. `SKIP_E2E=1 ./scripts/check_full.sh` exit 0 (./.tmp/stage13-check-full.log). Release size 15 801 784 bytes, −384 vs R11 (R13).
- Visual acceptance: the user checked the running app themselves (header `ViRShot 0.1.0-dev`, build line without the marker) and reported it as good; the headless screenshot harness was not re-run.
- Note for a future backlog item: `hotkeys::tests::already_registered_failure_reason_names_the_key_and_a_hint` reads `t!` without holding the locale mutex, so it can race with locale-mutating guarded tests (pre-existing, observed once during Stage 12; three later full runs green). Guarding that test is a separate task, not part of this one.
- Task archived to `.dumbspec/archive/2026-11-10_about-dialog/` on 2026-11-11 by the user's request (runtime acceptance done by the user). Still deferred to a Windows run: spec question 7 (link hand-off), 10 (OS close), 11 (min/max button visibility), and question 12 (the `-dev` detection on a real GitHub Actions tag build).