# Research: gui.rs split + update/ move

Repo: /home/apkawa/code/vibe-shot (`virshot`, bin-only root package). Findings gathered by research subagent, verified via reads/greps. No files modified.

## Current layout

- `src/` is flat: one module = one file, each with a sibling `<module>.tests.rs` attached via
  `#[cfg(test)] #[path = "...tests.rs"] mod tests;` at the end of the module file (17 attachments total;
  example `src/capture.rs:646-648`). Test files begin `use super::*;`.
- `src/gui.rs` = **3912 lines** (draft said ~3k), `src/gui.tests.rs` = **2850 lines**, ~150 test fns.
- Update trio (flat, all `pub(crate)`):
  - `src/update.rs` — 500 lines, tests `src/update.tests.rs` (877). Release-manifest contract:
    `UpdateError`, `ReleaseManifest`/`ReleaseFile`, `BuildInfo`, `current_build()`,
    `decide_update`, `version_higher`, RFC-3339 parsing, `next_check_deadline(now, Option<Duration>)`,
    `manifest_url(config::UpdateChannel)`, `fetch_manifest_text` (ureq), `CheckOutcome`, `UpdateInfo`,
    `run_check`, `spawn_check(channel, BuildInfo, Sender<CheckOutcome>, fetch)`.
  - `src/update_install.rs` — 815 lines, tests `src/update_install.tests.rs` (596). Download→verify→
    unpack→self-replace seam→GC: `InstallError`, base64/sha512 helpers, `verify_archive`,
    `unpack_archive` (zip/tar.gz), `download_archive`, `install_dir`, `install(&update::UpdateInfo, replace)`,
    `install_with`, `cleanup_leftovers`.
  - `src/update_notes.rs` — 203 lines, tests `src/update_notes.tests.rs` (251). GitHub release body for
    the dialog: `NotesError`, `ReleaseNotes`, `notes_url`, `release_page_url`, `parse_release_notes`,
    `fetch_release_notes`, `spawn_notes_fetch`.

## gui.rs feature areas (line ranges)

- 1–143 module doc; 144–187 imports.
- **Window setup**: 190–191 `INITIAL_WINDOW_WIDTH/HEIGHT`.
- **General tab helpers**: 197 `TRAY_POLL_TICK` (tray area); 206 `browse_start_dir`;
  219–276 placeholder accordion (`PLACEHOLDER_DESC_KEYS`, `placeholder_description`,
  `placeholder_reference_lines` ← `template::TOKENS`); 277–298 `format_label`/`position_label`;
  300–330 language (`LANGUAGE_OPTIONS`, `native_name`, `locale_code_of`);
  332–378 overlay-test chip (`OVERLAY_TEST_FILENAME`, `overlay_test_*`, `test_overlay_toast`).
- **Hotkey/bind capture**: 265–268 `HOTKEY_ROWS`/`row_label`; 412 `SUPPORTED_KEY_NAMES`;
  525–631 labels/tooltips/colors (`MOD_NAMES`, `WARNING_COLOR`, `INFO_COLOR`, `WARNING_ICON_SIZE`);
  635–649 `enum Tab`; 662–772 key capture (`supported_key_name`, `binding_label`, `mods_from_press`,
  `binding_from_press`, `probe_binding_from_press`, `clear_*`, `bind_button_label`);
  774–950 duplicate/save gate (`draft_savable`, `DuplicateKind`, `DuplicateBinding`,
  `duplicate_bindings`, warnings); 952–982 registration retry/merge;
  984–1090 capture state-machine decisions (`CaptureTransition`, `CaptureInput`,
  `capture_transition`, `pad_capture_toggle`, `PadCaptureChange`, `pad_capture_action`,
  `pad_capture_target`, `pad_field_label`).
- **Icons/textures**: 1092–1206 (`IconKind`, `StatusIcon`, `icon_for_status`, glyphs/colors,
  `warning_icon_texture`, `about_icon_texture`).
- **Bottom bar / close**: 1208–1266 (`is_dirty`, `BottomButtons`, `bottom_buttons_enabled`,
  `BOTTOM_BAR_PANEL_ID`, `cancel_draft`, `CloseAction`, `close_decision`).
- **Update UI**: contiguous block **1274–1826** — `UpdateStatus`, `UpdateDialogButton/Outcome`,
  check/install logic fns, `UpdateDialogWindow`, `UpdateDialogView`, `paint_update_dialog`,
  `update_check_selected_text`, `update_chip_geometry`, `update_discovery_chip`, `update_failure_chip`.
- 1831–1834 `pub(crate) enum Error`.
- **App state**: 1839–2005 `pub(crate) struct SettingsApp` (config, controller, capture state,
  mailbox, toast/overlay, tray, textures, about state, update fields incl. channels/deadlines).
- **impl SettingsApp** 2007–3072: `new`, binding helpers, registration helpers, `apply`,
  capture wiring (2224–2483), `general_tab` 2484–2722 (incl. update-interval/channel/auto-install
  controls 2579–2703), `browse_save_path`, `hotkeys_tab` 2760–2905, `overlay_tab` 2906–2977,
  `publish_overlay_test_chip`, update methods 2996–3072 (`open_update_dialog`, `dismiss_update_dialog`,
  `start_install`, `publish_update_chip`).
- **impl eframe::App** 3073–3691: `logic` (tray poll 3086–3134, About viewport 3136–3193,
  auto-update block 3195–3568, close decision 3480–3512, repaint-after), `clear_color`, `ui`
  (tab strip + bottom bar).
- **Window/host plumbing**: 3710–3754 `host_monitor_rects`, 3764–3795 `app_window_icon`,
  3796–3908 `pub(crate) fn run(...)`.
- 3910–3912 test attachment.

## External references to gui::

- `src/main.rs:518` — only `gui::run(...)` (other mentions are doc comments).
- `src/hotkeys.tests.rs:419,423` — `crate::gui::binding_label`.
- `src/i18n.tests.rs:114-115` — `gui::OVERLAY_TEST_FILENAME`, `gui::overlay_test_message`.
  After split these must point to the new submodule paths.
- gui.tests.rs maps onto areas: keyboard/binding ≈74–520, icons 523–575, bottom/close 577–1069,
  labels 1070–1133, capture transitions 1135–1175, pad 1208–1485, textures/icons 1487–1520,
  placeholders/overlay-test 1544–1685, registration retry 1699–1861, locale/language 1863–2076,
  update UI 2079–2850.

## Update trio callers

- main.rs: `mpsc::channel::<update::CheckOutcome>()` 513/538/564; `arm_startup_check` 582–591
  (`current_build`, `should_check_at_startup` (main.rs:150), `spawn_check(...fetch_manifest_text)`);
  GUI-less park loop 678–750 (`spawn_check`, `next_check_deadline`, `install` + restart tail 594–650);
  `cleanup_leftovers` on every start 275–276.
- gui.rs: fields 1922–2004; UI logic 1274–1826; methods 2996–3072; logic-pass 3195–3568.
- `update_notes::` used only from gui.rs (`open_update_dialog`, dialog paint).
- `update.rs::current_build()` depends on `crate::about::version_with_dev` and `crate::build::*`
  (shadow-rs) — stays reachable after the move (same crate).
- Restart contract: install success → `restart_requested: Arc<AtomicBool>` + `ViewportCommand::Close`;
  main tail runs restart (`self_replace::self_replace` seam passed from gui.rs:3048 / main park path).

## Build/test gates

- `scripts/check.sh`: `./cargo.sh check` → `cargo.sh test` (`SKIP_E2E=1` skips `cli_headless::`) →
  `cargo.sh clippy -- -D warnings`. Baseline on branch start: `./cargo.sh check` green.
- Cargo.toml: bin-only root package, no lib target, no `[[test]]` targets; `tests/unit/main.rs` is a
  placeholder (activates only when a lib exists). Formatting at commit time (rusty-hook chain).
- No new dependencies needed; ureq, self-replace, serde-saphyr, egui_commonmark already locked.

## Notes for design

- There is **no** `src/gui/` directory today; moving gui.rs → gui/mod.rs makes submodule files natural.
- `update_chip_geometry` locally duplicates capture's private monitor→corner mapping (tolerated per
  AGENTS.md KISS).
- `gui.rs` module doc (1–143) holds design notes (tray polling, toast ownership, update scheduling
  §4.7, close semantics §4.5) — needs a home in the split.
- All cross-references in main.rs use flat paths `update::`, `update_install::`, `update_notes::`;
  the move changes them to `update::install`/`update::notes` (or re-export in `update/mod.rs`).
