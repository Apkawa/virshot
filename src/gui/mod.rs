//! Settings window GUI (spec §4.4): the eframe/egui app on the MAIN thread —
//! three tabs (General / Hotkeys / Overlay), an in-app draft of `Settings`, and
//! a bottom row of `Apply | OK | Cancel` right-aligned at the very bottom
//! (§4.4): Apply saves the draft (`crate::config::save` + live hotkey
//! re-registration via `crate::hotkeys::HotkeysController`) and keeps the window open,
//! OK saves then closes for real, Cancel discards the draft and closes — a
//! discard never saves or publishes. Apply/OK also stay disabled while the
//! draft has duplicate triggers (§4.4, Stage 3): `crate::config::save` validates
//! nothing, so [`binding::duplicate_bindings`] rejects a repeated combo or
//! pad in the GUI before save, the colliding controls carry a Warning icon
//! bound to the control they describe (§4.4, Stage 10: keyboard warnings sit
//! LEFT of the bind button, pad warnings LEFT of the pad field —
//! [`binding::keyboard_warning`] and [`binding::pad_warning`] split the status
//! per control), and `apply()` refuses such a
//! draft as defense in depth. UI text is English (spec §2
//! i18n note).
//!
//! eframe 0.37 API facts verified in the locked source, not from memory:
//! - `NativeOptions::viewport` (`ViewportBuilder`): `with_title`,
//!   `with_inner_size` (720×520 per §4.4), `with_icon(Arc<IconData>)` —
//!   attached at creation, since Windows captures `ICON_BIG` there (eframe's
//!   `AppTitleIconSetter`) — plus the dialog chrome of §4.4:
//!   `with_minimize_button(false)` + `with_maximize_button(false)`. Close and
//!   resize stay enabled (bordered, resizable window).
//! - Widgets: `ComboBox::from_label/from_id_salt(...).selected_text(...).show_ui`,
//!   `ui.selectable_value`, `ui.checkbox`, `ui.text_edit_singleline`,
//!   `DragValue::range`, `ui.add_enabled(false, ...)` for disabled rows.
//! - General tab placeholder reference (spec §4.2, Stage 6): an
//!   `egui::CollapsingHeader` right under the Window template field, rows
//!   derived from `crate::template::TOKENS` through [`general::placeholder_reference_lines`].
//!   egui 0.37 has NO `CollapsibleGroup` (verified in the locked source: the
//!   collapsible widget is `src/containers/collapsing_header.rs`,
//!   `CollapsingHeader::new(text)` + `default_open(bool)` +
//!   `show(ui, |ui| ...)`), so the collapsing header IS the accordion; it stays
//!   collapsed by default because it is a reference, not a control.
//! - General tab Browse button (spec §4.4, Stage 14): the `rfd` blocking
//!   folder picker (`FileDialog::new().set_directory(..).pick_folder()`)
//!   started from `general::browse_start_dir`; cancel or a missing backend is `None` →
//!   WARN log, draft unchanged, app stays alive (see `general::browse_save_path`).
//! - Overlay tab test chip (spec §4.3, Stage 7): a `Test overlay` button right
//!   of the "Show overlay toast" checkbox, disabled while the checkbox is off
//!   (a disabled button needs the gate HERE — `overlay.enabled` is only
//!   consulted by `crate::capture::should_show`, and the test chip publishes directly
//!   through the hand-off handle). The click builds a success chip from the
//!   CURRENT draft ([`overlay::test_overlay_toast`]) with the real save wording
//!   ([`overlay::overlay_test_message`] + `crate::capture::saved_text`) — no capture, no file,
//!   no config write, no hotkey re-registration.
//! - Hotkeys rows (spec §4.4 v5.0): the bind field is a `Button` whose label is
//!   the current binding (`binding::binding_label`), and registration status is an icon
//!   with a tooltip instead of inline text. Verified in the locked egui 0.37
//!   source: `Button::new(atoms: impl IntoAtoms)` accepts any `Into<WidgetText>`
//!   (`&str`, `String`, `RichText`) and sizes via `min_size(Vec2)` (there is no
//!   `min_width`); `Response::on_hover_text(self, text) -> Self` consumes the
//!   response; and its tooltip is NOT shown on a disabled widget — the disabled
//!   rect row needs `Response::on_disabled_hover_text` instead.
//! - Hotkeys alignment (spec §4.4, Stage 2): the tab is ONE `egui::Grid`
//!   (`Grid::new(id_salt).num_columns(3).show(...)`, `src/grid.rs`) so the
//!   column widths are shared across rows — a grid per row sizes its own
//!   columns, which is why the rows never lined up. egui 0.37 has NO `Table`
//!   widget (verified: no table module in the locked source), so `Grid` is the
//!   alignment tool. Each nested scope (`ui.horizontal`, `ui.add_enabled_ui`)
//!   counts as ONE cell (`Placer::advance` advances per allocation). The status
//!   icon is painted BEFORE the action name, so it sits left of the bind button.
//! - Status icons: with our eframe features (`default_fonts`, no
//!   `monochrome_emoji_fonts`) the bundled fonts are Ubuntu-Light + Hack +
//!   egui-icons (5 logo glyphs only), and none of them contains `⚠`/`ℹ`
//!   (checked the cmaps). The warning icon is therefore the embedded
//!   triangle texture (`::icons::WARNING_ICON_PNG`, decoded via
//!   `::icons::rgba` — the `icons` CRATE, which the `gui::icons` child module
//!   shadows here; egui 0.37 core has no PNG loader, same shape as
//!   `crates/overlay/src/content.rs`), uploaded once at window creation;
//!   a decode failure logs and the row falls back to the colored ASCII
//!   `!`. The info icon stays a colored ASCII `i` via `RichText::color`.
//! - Bind-by-press reads `ui.input(|i| i.events())` `Event::Key` presses
//!   (spec §4.4: click field → capture mode); Escape cancels capture mode;
//!   Backspace clears the keyboard half of the row (key+mods emptied, pad kept)
//!   and ends capture; unsupported names are ignored. While capturing, live
//!   hotkey registrations
//!   are paused via `crate::hotkeys::HotkeysController::pause`/`restore` (Stage 18,
//!   spec §4.4 v5.5) so a press of a live combo reaches this window instead of
//!   firing the screenshot action; restore re-registers the APPLIED settings.
//! - Pad-field bind-by-press (spec §4.1 + §4.4, Stage 5): clicking the pad field
//!   starts capture for that row; the pump suppresses pad shots while ANY field
//!   is capturing and commits the FIRST DOWN event into `crate::gamepad::PadCapture`,
//!   so `logic` drains the mailbox into the draft notation — the GUI never
//!   receives a callback, only the mailbox. The notation IS the human-readable
//!   name: the device-info label of the pressed button (`XboxX`), or the raw
//!   `unknown:<N>` fallback when the device reports no label (v0 devices, §9);
//!   system buttons keep `Guide` / `Share`. The pad button shows the stored
//!   notation verbatim, so the field reads back exactly what config will hold;
//!   no separate label hint exists. On a host without GameInput the field
//!   renders as a disabled placeholder (Linux gilrs is out of scope).

mod app;
pub(crate) mod binding;
mod chrome;
mod general;
mod hotkeys;
mod icons;
pub(crate) mod overlay;
mod update;
mod window;

pub(crate) use app::SettingsApp;
pub(crate) use window::run;
