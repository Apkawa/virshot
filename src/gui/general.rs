//! General-tab functionality (spec §4.4): the language dropdown table and its
//! locale normalization, the placeholder reference rows derived from
//! `template::TOKENS`, the format labels, the Browse folder-picker start rule,
//! the update-interval dropdown's selected text, and the update row the tab
//! paints — plus the Windows-only autostart row at the bottom of the tab
//! (task `2026-11-13_autostart`) and its disabled non-Windows placeholder —
//! plus the `SettingsApp` methods that paint the tab and open the
//! picker. Extracted from `gui/mod.rs` (GUI refactor); the save-gate
//! tooltips, `Tab`, the bottom bar, and the hotkeys / overlay wiring live in
//! `gui/app.rs`, the corner label (`position_label`) in `gui/overlay.rs`, as do
//! the update-state fields and the check/install pump — the update decisions
//! and dialog themselves live in `gui/update.rs`, and this tab only asks them.

use std::path::Path;
use std::path::PathBuf;

use crate::config;
use crate::config::Format;
use crate::config::Settings;
use crate::template::TOKENS;

use super::SettingsApp;
use super::icons;
use super::update::ForcedCheckStatus;
use super::update::check_now_button_enabled;
use super::update::forced_check_status_shown;
use super::update::install_button_enabled;
use super::update::install_failure_hint;
use super::update::update_row_visible;

/// Start directory for the Browse folder picker (spec §4.4): the draft
/// `save_path` when it exists on disk, else `<home>/Pictures`
/// (`%USERPROFILE%\Pictures` on Windows, `~/Pictures` on Linux). With no home
/// directory the start is the cwd-relative `Pictures`, mirroring
/// `config::default_save_path`'s fallback. The existence check is a plain
/// `is_dir()` on the draft path — no dialog is involved, so the rule is
/// unit-tested against a real scratch directory.
fn browse_start_dir(save_path: &Path, home: Option<&Path>) -> PathBuf {
    if save_path.is_dir() {
        save_path.to_path_buf()
    } else {
        home.map(|home| home.join("Pictures"))
            .unwrap_or_else(|| PathBuf::from("Pictures"))
    }
}

/// Catalog keys for the placeholder descriptions (`general.placeholder_desc_*`).
/// Token names and key suffixes differ (`t`→`title`, `n`→`counter`,
/// `y`→`year_short`), so the mapping is spelled out; the `%token` notation
/// itself is config notation (spec §4.2) and stays untranslated.
const PLACEHOLDER_DESC_KEYS: [(&str, &str); 11] = [
    ("Y", "general.placeholder_desc_year"),
    ("y", "general.placeholder_desc_year_short"),
    ("mo", "general.placeholder_desc_month"),
    ("d", "general.placeholder_desc_day"),
    ("H", "general.placeholder_desc_hour"),
    ("M", "general.placeholder_desc_minute"),
    ("S", "general.placeholder_desc_second"),
    ("iso", "general.placeholder_desc_iso"),
    ("t", "general.placeholder_desc_title"),
    ("n", "general.placeholder_desc_counter"),
    ("ext", "general.placeholder_desc_ext"),
];

/// One token's localized description; an unlisted token falls back to the
/// English `template::TOKENS` text rather than rendering a dotted key.
fn placeholder_description(token: &str, english: &str) -> String {
    match PLACEHOLDER_DESC_KEYS
        .iter()
        .find(|(name, _)| *name == token)
    {
        Some((_, key)) => rust_i18n::t!(*key).into_owned(),
        None => english.to_owned(),
    }
}

/// Placeholder reference rows for the General tab accordion (spec §4.2): one
/// `(token, description)` line per `template::TOKENS` entry, token spelled with
/// its `%`. The table is the single source of truth — it is the very list
/// `template::render` validates against — so the UI states exactly the tokens a
/// template may use and cannot drift from the renderer. Descriptions are
/// catalog lookups (spec §2); tokens stay config notation. Rows are rebuilt
/// each UI frame: 11 short strings, well under the §5 budget.
pub(crate) fn placeholder_reference_lines() -> Vec<(String, String)> {
    TOKENS
        .iter()
        .map(|(name, description)| {
            (
                format!("%{name}"),
                placeholder_description(name, description),
            )
        })
        .collect()
}

/// GUI labels for formats (config names are `png|jpg|webp`).
fn format_label(format: Format) -> &'static str {
    match format {
        Format::Png => "png",
        Format::Jpeg => "jpg",
        Format::Webp => "webp",
    }
}

/// Language dropdown options (spec §2, Stage 2b): config code + NATIVE name.
/// The array order IS the dropdown order, and `en` is first because it is the
/// spec §7 default. Native names are endonyms — they are never localized
/// ("Русский" stays Russian in every locale), so they are constants here,
/// not catalog keys; only the row label is (`general.language_label`).
pub(crate) const LANGUAGE_OPTIONS: [(&str, &str); 6] = [
    ("en", "English"),
    ("ru", "Русский"),
    ("zh-CN", "简体中文"),
    ("es", "Español"),
    ("ja", "日本語"),
    ("ko", "한국어"),
];

/// Native display name of a config language code. An unknown or empty code
/// mirrors `config::language_code`'s lenient normalization by showing the
/// English endonym — the code normalizes to `en` when the draft is applied.
fn native_name(code: &str) -> &str {
    LANGUAGE_OPTIONS
        .iter()
        .find(|(option_code, _)| *option_code == code)
        .map(|(_, native)| *native)
        .unwrap_or(LANGUAGE_OPTIONS[0].1)
}

/// Locale code for `rust_i18n::set_locale` from a draft: a thin wrapper over
/// `config::language_code`, the same EXACT-match normalization
/// `config::load_or_default` applies on load (spec §7), so a dropdown value
/// and a hand-edited config switch identically on Apply.
pub(crate) fn locale_code_of(settings: &Settings) -> &'static str {
    config::language_code(&settings.general.language)
}

/// Selected text of the interval dropdown: `never` is a decision word —
/// localized (spec §4.4); the intervals are config literals shown verbatim
/// (`1h`/`1d`/`1w`), exactly like the Format dropdown's `png`/`jpg`/`webp`
/// option labels — technical values stay unlocalized.
fn update_check_selected_text(check: config::UpdateCheck) -> String {
    match check {
        config::UpdateCheck::Never => rust_i18n::t!("general.update_check_never").into_owned(),
        config::UpdateCheck::Hour => "1h".to_owned(),
        config::UpdateCheck::Day => "1d".to_owned(),
        config::UpdateCheck::Week => "1w".to_owned(),
    }
}

/// Label of the autostart checkbox (catalog `general.autostart_label`, task
/// `2026-11-13_autostart`). A catalog lookup helper, like
/// `binding::gamepad_placeholder_label()`: the wording is pinned by tests
/// against the shipped catalog, not against a literal in the paint code.
fn autostart_label() -> String {
    rust_i18n::t!("general.autostart_label").into_owned()
}

/// Text of the disabled autostart placeholder on hosts without the HKCU `Run`
/// registry (catalog `general.autostart_windows_only_note` — naming mirrors
/// `gamepad.windows_only_note`, same Windows-only-option convention).
fn autostart_placeholder_label() -> String {
    rust_i18n::t!("general.autostart_windows_only_note").into_owned()
}

impl SettingsApp {
    pub(crate) fn general_tab(&mut self, ui: &mut egui::Ui) {
        // Language dropdown (spec §2 live locale switch, Stage 2b): FIRST row
        // of the tab. The draft stores the canonical config code (Apply
        // persists it and switches the locale — see `apply`), so `is_dirty`
        // sees the change like any other draft field. Selected text and items
        // show NATIVE names (`LANGUAGE_OPTIONS`), the row label is a catalog
        // lookup.
        egui::ComboBox::from_label(rust_i18n::t!("general.language_label"))
            .selected_text(native_name(&self.draft.general.language))
            .show_ui(ui, |ui| {
                for (code, native) in LANGUAGE_OPTIONS {
                    if ui
                        .selectable_label(self.draft.general.language == code, native)
                        .clicked()
                    {
                        self.draft.general.language = code.to_owned();
                    }
                }
            });

        // The save-path row is rendered before the draft borrow is taken for
        // the rest of the tab: Browse rewrites `draft.general.save_path`
        // after the dialog returns (spec §4.4 — Apply persists it as usual).
        ui.label(rust_i18n::t!("general.save_path_label"));
        let mut browse_clicked = false;
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.draft.general.save_path);
            browse_clicked = ui.button(rust_i18n::t!("general.browse_button")).clicked();
        });
        if browse_clicked {
            self.browse_save_path();
        }

        let general = &mut self.draft.general;

        ui.label(rust_i18n::t!("general.full_screen_label"));
        ui.text_edit_singleline(&mut general.file_full_screen);

        ui.label(rust_i18n::t!("general.window_label"));
        ui.text_edit_singleline(&mut general.file_window);

        // Placeholder reference (spec §4.2, Stage 6): the token table rendered
        // straight from `template::TOKENS`, so the tab states exactly what
        // `template::render` accepts and never duplicates the list. Collapsed
        // by default (a reference, not a control); it is part of the tab's
        // vertical layout — no extra frame, no scrolling.
        egui::CollapsingHeader::new(rust_i18n::t!("general.placeholder_reference_header"))
            .id_salt("template_placeholder_reference")
            .default_open(false)
            .show(ui, |ui| {
                for (token, description) in placeholder_reference_lines() {
                    ui.horizontal(|ui| {
                        ui.monospace(token);
                        ui.label(format!("— {description}"));
                    });
                }
            });

        egui::ComboBox::from_label(rust_i18n::t!("general.format_label"))
            .selected_text(format_label(general.format))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut general.format, Format::Png, "png");
                ui.selectable_value(&mut general.format, Format::Jpeg, "jpg");
                ui.selectable_value(&mut general.format, Format::Webp, "webp");
            });

        ui.horizontal(|ui| {
            ui.add_enabled(
                general.format == Format::Jpeg,
                egui::DragValue::new(&mut general.jpeg_quality).range(1..=100),
            );
            ui.label(rust_i18n::t!("general.jpeg_quality_label"));
        });

        ui.horizontal(|ui| {
            ui.add_enabled(
                general.format == Format::Webp,
                egui::DragValue::new(&mut general.webp_quality).range(1..=100),
            );
            ui.label(rust_i18n::t!("general.webp_quality_label"));
        });

        // ── «Обновление» group (spec §4.4/§4.7, task 2026-11-11 Stages 6-9) ──
        // Draft edits participate in `is_dirty` (Settings derives PartialEq,
        // so the General fields added by Stage 2 are covered by the existing
        // machinery) and persist ONLY through `apply()` — no bypass. Behavior
        // reads APPLIED settings (§4.4): an unsaved interval/channel switch
        // cannot change checks, the row, or the install path until Apply.
        ui.separator();
        ui.label(rust_i18n::t!("update.section_title"));

        egui::ComboBox::from_label(rust_i18n::t!("update.check_label"))
            .selected_text(update_check_selected_text(general.update_check))
            .show_ui(ui, |ui| {
                // `Never` is a decision word — localized; the intervals are
                // config literals shown verbatim, exactly like the Format
                // dropdown's `png`/`jpg`/`webp` option labels.
                ui.selectable_value(
                    &mut general.update_check,
                    config::UpdateCheck::Never,
                    rust_i18n::t!("general.update_check_never"),
                );
                ui.selectable_value(&mut general.update_check, config::UpdateCheck::Hour, "1h");
                ui.selectable_value(&mut general.update_check, config::UpdateCheck::Day, "1d");
                ui.selectable_value(&mut general.update_check, config::UpdateCheck::Week, "1w");
            });

        // Forced check (spec §4.7), under the interval dropdown: it ignores the
        // interval — `Never` still checks — and never moves or re-arms the
        // scheduled deadline. Paint records the press only; `logic` spawns the
        // check with the APPLIED channel, so an unsaved channel switch cannot
        // redirect it (§4.4). The button is disabled while a check is in flight.
        // Stage 11 sub-item 2: the button and its RESULT line share one
        // horizontal row — the line shows ONLY the last forced outcome: the
        // localized «Обновлений нет» after an up-to-date check, or the
        // verbatim English `UpdateError` Display after a failure (never
        // localized, `capture::error_display` precedent, in the same warning
        // styling as the install-failure hint). Scheduled results keep using
        // the chip and the General row; nothing renders while a forced check
        // is in flight (`forced_check_status_shown`).
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    check_now_button_enabled(self.check_in_flight),
                    egui::Button::new(rust_i18n::t!("update.check_now_button")),
                )
                .clicked()
            {
                self.check_now_requested = true;
            }
            if let Some(status) = forced_check_status_shown(
                self.forced_check_status.as_ref(),
                self.forced_check_in_flight,
            ) {
                match status {
                    // Technical failure text stays English verbatim (no
                    // locale key) and shares the warning styling of
                    // `install_failure_hint` below.
                    ForcedCheckStatus::Failed(message) => {
                        ui.label(
                            egui::RichText::new(message.as_str())
                                .color(icons::WARNING_COLOR)
                                .strong(),
                        );
                    }
                    // The success line is a plain localized result, not an error.
                    ForcedCheckStatus::NoUpdates => {
                        ui.label(rust_i18n::t!("update.no_updates_available"));
                    }
                }
            }
        });

        egui::ComboBox::from_label(rust_i18n::t!("general.update_channel_label"))
            .selected_text(match general.update_channel {
                config::UpdateChannel::Stable => "stable",
                config::UpdateChannel::Nightly => "nightly",
            })
            .show_ui(ui, |ui| {
                // Channel values are release-tag names — config literals, not
                // decision words, so `stable`/`nightly` stay unlocalized.
                ui.selectable_value(
                    &mut general.update_channel,
                    config::UpdateChannel::Stable,
                    "stable",
                );
                ui.selectable_value(
                    &mut general.update_channel,
                    config::UpdateChannel::Nightly,
                    "nightly",
                );
            });

        ui.checkbox(
            &mut general.auto_install,
            rust_i18n::t!("general.auto_install_label"),
        );

        // «Update available» row (spec §4.7): the dialog-path UI. With
        // auto-install ON a discovery never sits held — install starts at
        // discovery — so this row is what the OFF path offers: open the
        // update dialog while Idle, read the verbatim failure text after a
        // failed install, and stay disabled while Installing. Visibility is
        // `update_row_visible` (a held discovery is enough): a discovery can
        // come from the forced check, which works with ANY interval (§4.7), so
        // neither the interval nor dev-ness gates the row anymore.
        if update_row_visible(self.update.as_ref()) {
            let version = self
                .update
                .as_ref()
                .map(|info| info.version.as_str())
                .unwrap_or("");
            let failure = install_failure_hint(&self.update_status).map(str::to_owned);
            let enabled = install_button_enabled(&self.update_status);
            let mut open_dialog = false;
            ui.horizontal(|ui| {
                ui.label(rust_i18n::t!("general.update_available_label"));
                ui.label(version);
                match failure {
                    // A failed install replaces the button with the verbatim
                    // English error text (no locale key for technical
                    // failures) in the warning color already used for status
                    // icons — the message itself is the evidence.
                    Some(message) => {
                        ui.label(
                            egui::RichText::new(message)
                                .color(icons::WARNING_COLOR)
                                .strong(),
                        );
                    }
                    None => {
                        open_dialog = ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(rust_i18n::t!("general.install_update_button")),
                            )
                            .clicked();
                    }
                }
            });
            if open_dialog {
                self.open_update_dialog();
            }
        }

        // ── «Autostart» group (spec §4.4, task 2026-11-13_autostart) ──
        // The bottom group of the tab, below the Update rows. The draft flag
        // is Windows-only (`config::General::autostart`, `#[cfg(windows)]`),
        // so the row reads/writes it through the dual accessors in
        // `config.rs` instead of naming the field: `src/gui/` has no
        // compile-time `#[cfg]` and must compile on Linux, exactly like the
        // pad field's runtime `cfg!(windows)` split in `gui/hotkeys.rs:547`.
        // Draft semantics are untouched — the write lands in `draft.general`
        // like any other field, so dirty detection stays the plain
        // draft-vs-applied `PartialEq` (`chrome::is_dirty`), no special case.
        // Apply persists it (`config::save` on Windows) and syncs the registry
        // from the saved draft (`autostart::sync` in `gui/app.rs::apply`); a
        // manual config edit lands in the registry at startup (`main.rs`).
        ui.separator();
        if cfg!(windows) {
            let mut autostart = self.draft.general.autostart();
            ui.checkbox(&mut autostart, autostart_label());
            // Write-back of the (possibly toggled) value; on a frame with no
            // press it writes the value it just read, so the draft is a no-op.
            self.draft.general.set_autostart(autostart);
        } else {
            // Disabled placeholder, the exact mechanism of the pad-field
            // placeholder (`ui.add_enabled(false, …)`, `gui/hotkeys.rs:574-577`):
            // a disabled widget that never reacts to clicks, so the flag stays
            // `false` and the registry is never touched on Linux.
            let mut placeholder = false;
            ui.add_enabled(
                false,
                egui::Checkbox::new(&mut placeholder, autostart_placeholder_label()),
            );
        }
    }

    /// Browse → native folder picker (spec §4.4, `rfd` approved by the user
    /// 2026-11-03). API facts verified in the locked rfd 0.17.2 source, not
    /// from memory: the BLOCKING `rfd::FileDialog` builder —
    /// `FileDialog::new()`, `set_directory<P: AsRef<Path>>(self, path)`,
    /// `pick_folder(self) -> Option<PathBuf>` (`src/file_dialog.rs`) — needs
    /// NO async/tokio feature: default features are `xdg-portal` + `wayland`,
    /// which pull `pollster` (unix-only) for blocking-over-portal; Windows
    /// uses `windows-sys` common controls (`src/backend/win_cid.rs`), Linux
    /// the XDG desktop portal with a zenity fallback
    /// (`src/backend/xdg_desktop_portal.rs`). Every backend failure path
    /// (libdbus missing, portal missing, zenity missing) returns `None` —
    /// rfd never panics there — so no unwraps are needed here.
    ///
    /// The dialog is modal and blocking: the UI pass waits for it (the tray
    /// poll tick resumes afterwards). Cancel and "no backend" are both `None`,
    /// so the draft is left untouched and the app stays alive; only a picked
    /// folder rewrites `draft.general.save_path` (Apply persists it, §4.4).
    pub(crate) fn browse_save_path(&mut self) {
        let start = browse_start_dir(
            Path::new(&self.draft.general.save_path),
            config::home_dir().as_deref(),
        );

        match rfd::FileDialog::new().set_directory(start).pick_folder() {
            Some(folder) => {
                self.draft.general.save_path = folder.display().to_string();
                tracing::info!("Save path picked: {}", self.draft.general.save_path);
            }
            None => tracing::warn!(
                "folder picker unavailable (cancelled, or no native backend on this host)"
            ),
        }
    }
}

#[cfg(test)]
#[path = "general.tests.rs"]
mod tests;
