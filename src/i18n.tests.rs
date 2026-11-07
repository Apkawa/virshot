//! Unit tests for the i18n wiring (spec §2): the process-wide default locale,
//! the English catalog resolving to the EXACT user-visible strings the source
//! uses today, the `en` fallback for a locale without entries, and the shape
//! of the single catalog file `./locales/app.yml`.
//!
//! These tests live in a sibling file attached to the crate root (`main.rs`)
//! because the `rust-i18n` init macro is process-wide wiring, not module
//! logic: `t!`/`locale()` are the crate-level API the whole UI calls, so the
//! contract is pinned at the place that initialises it.
//!
//! RED by design until the i18n stage lands: with no `i18n!` init macro there
//! is no `_rust_i18n_t!` in this crate, so every `t!` call below is a compile
//! error, and `./locales/app.yml` does not exist yet.
//!
//! Determinism: no capture, no display, no clock. `rust_i18n::locale()` is a
//! process-wide global, so every test that reads or changes it holds ONE
//! mutex — otherwise a concurrent test on another thread could observe a
//! locale another test set (rust-i18n caches the locale per thread, so
//! restoring inside the guarded thread is sufficient).

use super::*;

use std::sync::Mutex;
use std::sync::MutexGuard;

/// Serializes every test that reads or writes the process-wide locale.
static LOCALE_LOCK: Mutex<()> = Mutex::new(());

/// Take the locale lock; a poisoned lock (a panic in another locale test) is
/// recovered rather than unwrapped, like every other mailbox in this crate.
fn locale_guard() -> MutexGuard<'static, ()> {
    LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The English catalog must reproduce the current UI wording byte-for-byte.
/// Every expected string below was read out of the source, not invented; the
/// source location is listed in the task report.
#[test]
fn catalog_en_entries_resolve_to_original_english_text() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Settings tabs (`gui::Tab::label`).
    assert_eq!(rust_i18n::t!("general.tab_general"), "General");
    assert_eq!(rust_i18n::t!("general.tab_hotkeys"), "Hotkeys");
    assert_eq!(rust_i18n::t!("general.tab_overlay"), "Overlay");

    // General tab rows (`gui::SettingsApp::general_tab`).
    assert_eq!(rust_i18n::t!("general.save_path_label"), "Save path");
    assert_eq!(
        rust_i18n::t!("general.full_screen_label"),
        "Full-screen template"
    );
    assert_eq!(rust_i18n::t!("general.window_label"), "Window template");
    assert_eq!(
        rust_i18n::t!("general.placeholder_reference_header"),
        "Placeholder reference"
    );
    assert_eq!(rust_i18n::t!("general.format_label"), "Format");
    assert_eq!(rust_i18n::t!("general.jpeg_quality_label"), "JPEG quality");
    assert_eq!(
        rust_i18n::t!("general.webp_quality_label"),
        "WebP quality (WebP is lossless in MVP — quality not applied yet)"
    );

    // `%t` default title for a full-screen target (`template::FULLSCREEN_LITERAL`).
    assert_eq!(
        rust_i18n::t!("general.template_title_default"),
        "fullscreen"
    );

    // Bottom row (`gui::SettingsApp::bottom_bar`) + window title.
    assert_eq!(rust_i18n::t!("bottom.cancel"), "Cancel");
    assert_eq!(rust_i18n::t!("bottom.ok"), "OK");
    assert_eq!(rust_i18n::t!("bottom.apply"), "Apply");
    assert_eq!(rust_i18n::t!("bottom.unsaved_changes"), "Unsaved changes");
    assert_eq!(rust_i18n::t!("bottom.window_title"), "virshot — Settings");

    // Hotkeys tab constants + registration failure reasons (`hotkeys::failure_reason`).
    assert_eq!(
        rust_i18n::t!("hotkey.bind_hint"),
        "Esc to cancel, Backspace to clear"
    );
    assert_eq!(
        rust_i18n::t!("hotkey.unassigned"),
        "unbound — click to bind"
    );
    assert_eq!(rust_i18n::t!("hotkey.pad_label"), "pad — click to bind");
    assert_eq!(
        rust_i18n::t!("hotkey.already_registered", combo = "Alt+PrintScreen"),
        "already registered by another application (Alt+PrintScreen is held elsewhere) — free that key or pick another"
    );
    assert_eq!(
        rust_i18n::t!("hotkey.registration_failed", reason = "backend refused"),
        "registration failed: backend refused"
    );

    // Overlay tab rows (`gui::SettingsApp::overlay_tab`).
    assert_eq!(rust_i18n::t!("overlay.enabled_label"), "Show overlay toast");
    assert_eq!(rust_i18n::t!("overlay.position_label"), "Position");
    assert_eq!(rust_i18n::t!("overlay.duration_label"), "Duration");
    assert_eq!(
        rust_i18n::t!("overlay.on_all_monitors_label"),
        "Show on all monitors"
    );
    assert_eq!(rust_i18n::t!("overlay.test_button"), "Test overlay");

    // Overlay tab test chip text: `gui::overlay_test_message` renders the
    // success wording with the fixed test filename, so `overlay.test_message`
    // IS `toast.saved` applied to `gui::OVERLAY_TEST_FILENAME`.
    assert_eq!(
        rust_i18n::t!("toast.saved", file_name = gui::OVERLAY_TEST_FILENAME),
        gui::overlay_test_message(gui::OVERLAY_TEST_FILENAME)
    );

    // Toast chip texts (`capture::saved_text`, `capture::Error::Save`).
    assert_eq!(
        rust_i18n::t!("toast.saved", file_name = "shot.png"),
        "Saved: shot.png"
    );
    assert_eq!(
        rust_i18n::t!(
            "toast.save_failed",
            path = "/tmp/shot.png",
            reason = "disk full"
        ),
        "failed to save /tmp/shot.png: disk full"
    );

    // Tray menu labels and tooltip (`tray::take_screenshot_label()`,
    // `tray::settings_label()`, `tray::exit_label()`, `tray::tooltip()`).
    assert_eq!(rust_i18n::t!("tray.take_screenshot"), "Take screenshot");
    assert_eq!(rust_i18n::t!("tray.settings"), "Settings");
    assert_eq!(rust_i18n::t!("tray.exit"), "Exit");
    assert_eq!(rust_i18n::t!("tray.tooltip"), "virshot");

    // Restore the default locale so a later test never sees a mutated global.
    rust_i18n::set_locale("en");
}

/// The process-wide default locale is English: the UI shipped as English and
/// the catalog's `en` block holds that original wording.
#[test]
fn locale_default_is_english() {
    let _guard = locale_guard();
    assert_eq!(&*rust_i18n::locale(), "en");
}

/// A locale with no entries yet must still render English: init is expected to
/// declare `fallback = "en"`, so a miss falls back instead of echoing the key.
#[test]
fn missing_key_falls_back_to_en_via_fallback() {
    let _guard = locale_guard();
    // Stage 4 gives every shipped locale a full catalog, so the fallback
    // probe uses a locale OUTSIDE the catalog (`de`): a miss must fall back
    // to `en` instead of echoing the key.
    rust_i18n::set_locale("de");
    let text = rust_i18n::t!("general.tab_general");
    // Restore BEFORE the assertion so a failure never leaves `de` installed.
    rust_i18n::set_locale("en");

    assert_eq!(text, "General");
}

/// The catalog is ONE file at the repo root, rust-i18n version 2 format, with
/// an English locale block. The Russian block lands in Stage 3 and the
/// zh-CN/es/ja/ko blocks in Stage 4; this completeness test is extended there
/// to require every locale block that ships.
#[test]
fn catalog_is_one_file_with_version_two() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("locales")
        .join("app.yml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{} must exist and be readable: {err}", path.display()));

    assert!(
        text.lines().any(|line| line.trim() == "_version: 2"),
        "locales/app.yml must declare a `_version: 2` line"
    );
    assert!(
        text.lines()
            .any(|line| line.trim_start().starts_with("en:")),
        "locales/app.yml must have an `en:` locale block"
    );
    // `ru` landed in Stage 3 and zh-CN/es/ja/ko in Stage 4; per-key locale
    // completeness is pinned by `every_catalog_key_has_all_six_locale_translations`.
}

/// Completeness (spec §2): EVERY dotted key declared in `locales/app.yml`
/// must resolve to a real localized string under `en`. rust-i18n echoes the
/// key text itself on a miss, so a key declared without an `en:` block (or
/// with a typo in its locale nesting) fails here. Keys with `%{...}`
/// placeholders are called without args: the render must still be a real
/// localized string — non-empty and never the key itself. The catalog is
/// parsed with plain line walking (no YAML dependency): a top-level line
/// with no leading whitespace that ends with `:` declares a dotted key, and
/// it counts only when at least one nested (indented) locale line follows.
/// `_version: 2` and comments are not key lines.
#[test]
fn every_catalog_key_resolves_under_english() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("locales")
        .join("app.yml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{} must exist and be readable: {err}", path.display()));

    let mut keys: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // A nested locale line confirms the pending top-level key.
            if let Some(key) = pending.take() {
                keys.push(key);
            }
            continue;
        }
        // Top-level line: a dotted key ends with ':'; `_version: 2` does not.
        pending = if line.ends_with(':') {
            Some(line.trim_end_matches(':').to_owned())
        } else {
            None
        };
    }

    assert_eq!(
        keys.len(),
        76,
        "locales/app.yml must declare exactly 76 shipped keys (extended when Stage 3 adds keys)"
    );

    for key in &keys {
        let rendered = rust_i18n::t!(key.as_str()).into_owned();
        assert!(
            !rendered.is_empty(),
            "key `{key}` must render a non-empty string under en"
        );
        assert_ne!(rendered, *key, "key `{key}` must resolve, not echo itself");
        assert!(
            !rendered.contains(&format!(".{key}")),
            "key `{key}` must not fall back to a dotted-key path inside the text"
        );
    }

    rust_i18n::set_locale("en");
}

/// The plain line-walk parser above, extended (Stage 3) to attach every
/// nested `locale: value` line to its top-level dotted key — same shape, no
/// YAML dependency. Values are kept EXACTLY as written, quotes included:
/// the quoting rule is part of the file contract, and byte-for-byte
/// equality is what "copied, not translated" means. Shipped notation
/// values (`OK`, `virshot`) stay untranslated and are written quoted under
/// `ru`, so the raw comparison accepts them.
fn catalog_entries(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut entries: Vec<(String, Vec<(String, String)>)> = Vec::new();
    let mut current: Option<(String, Vec<(String, String)>)> = None;
    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            // Nested locale line; locale names never contain ':', so the
            // first split is the locale, the rest is the raw value.
            if let Some((locale, raw)) = line.trim_start().split_once(':')
                && let Some(entry) = current.as_mut()
            {
                entry.1.push((locale.to_owned(), raw.trim().to_owned()));
            }
            continue;
        }
        if let Some(entry) = current.take() {
            entries.push(entry);
        }
        // Top-level line: a dotted key ends with ':'; `_version: 2` does not.
        current = if line.ends_with(':') {
            Some((line.trim_end_matches(':').to_owned(), Vec::new()))
        } else {
            None
        };
    }
    if let Some(entry) = current.take() {
        entries.push(entry);
    }
    entries
}

/// The locale value as text, with the YAML double quotes unwrapped.
fn unquoted(raw: &str) -> &str {
    raw.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(raw)
}

/// Placeholder names `%{...}` inside a raw catalog value, sorted — rust-i18n
/// substitutes by NAME, so en and ru can be compared as sets.
fn placeholder_names(raw: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = raw;
    while let Some(start) = rest.find("%{") {
        let body = &rest[start + 2..];
        let Some(end) = body.find('}') else {
            break;
        };
        names.push(body[..end].to_owned());
        rest = &body[end + 1..];
    }
    names.sort();
    names
}

/// The catalog file, read from the repo root (same path as the shape test).
fn catalog_text() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("locales")
        .join("app.yml");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{} must exist and be readable: {err}", path.display()))
}

/// Every locale block shipped by the catalog (Stage 2 dropdown + Stage 3/4).
const SUPPORTED_LOCALES: [&str; 6] = ["en", "ru", "zh-CN", "es", "ja", "ko"];

/// Keys whose values are notation, not prose: they must stay verbatim in
/// every locale (`OK`, `virshot`, the ` ms` unit suffix). Only these keys
/// may repeat the raw `en` value byte-for-byte; every other key must be
/// translated. Quoted notation values (`ru: "OK"`) already differ raw.
const NOTATION_KEYS: [&str; 3] = ["bottom.ok", "tray.tooltip", "overlay.duration_suffix"];

/// Completeness (spec §2, Stage 3/4): EVERY shipped key must carry nested
/// `en:`, `ru:`, `zh-CN:`, `es:`, `ja:` and `ko:` lines, each value non-empty,
/// and — for every non-notation locale — not a byte-for-byte copy of the
/// `en:` line: a translated key differs from English. Shipped notation
/// values (`OK`, `virshot`, ` ms`) stay untranslated and are exempt.
#[test]
fn every_catalog_key_has_all_six_locale_translations() {
    let entries = catalog_entries(&catalog_text());
    assert_eq!(
        entries.len(),
        76,
        "locales/app.yml must declare exactly 76 shipped keys"
    );

    for (key, locales) in &entries {
        let en_raw = locales
            .iter()
            .find(|(locale, _)| locale == "en")
            .map(|(_, raw)| raw.as_str())
            .unwrap_or_else(|| panic!("key `{key}` must have a nested `en:` line"));
        for locale in SUPPORTED_LOCALES.iter().skip(1) {
            let raw = locales
                .iter()
                .find(|(entry_locale, _)| entry_locale == locale)
                .map(|(_, raw)| raw.as_str())
                .unwrap_or_else(|| panic!("key `{key}` must have a nested `{locale}:` line"));
            assert!(
                !unquoted(raw).is_empty(),
                "key `{key}` must have a non-empty {locale} value"
            );
            if !NOTATION_KEYS.contains(&key.as_str()) {
                assert_ne!(
                    raw, en_raw,
                    "key `{key}` must be translated, not copied verbatim from en ({locale})"
                );
            }
        }
    }
}

/// Interpolation (spec §2): every `%{name}` placeholder present in an `en`
/// value must appear — with the identical name — in every locale value,
/// because rust-i18n substitutes placeholders by name. Sorted-set comparison.
#[test]
fn placeholder_names_survive_in_every_locale() {
    let entries = catalog_entries(&catalog_text());

    let mut checked = 0usize;
    for (key, locales) in &entries {
        let en_raw = locales
            .iter()
            .find(|(locale, _)| locale == "en")
            .map(|(_, raw)| raw.as_str())
            .unwrap_or_else(|| panic!("key `{key}` must have a nested `en:` line"));
        let en_names = placeholder_names(en_raw);
        if en_names.is_empty() {
            continue;
        }
        checked += 1;
        for locale in SUPPORTED_LOCALES.iter().skip(1) {
            let raw = locales
                .iter()
                .find(|(entry_locale, _)| entry_locale == locale)
                .map(|(_, raw)| raw.as_str())
                .unwrap_or_else(|| panic!("key `{key}` must have a nested `{locale}:` line"));
            assert_eq!(
                placeholder_names(raw),
                en_names,
                "key `{key}` must keep the same placeholder names in {locale}"
            );
        }
    }
    assert!(
        checked > 0,
        "the catalog must contain interpolated keys for this test to mean anything"
    );
}

/// Under the `ru` locale the tab headers render Russian (spec §7 dropdown).
/// `ru` is restored BEFORE the assertions so a failure never leaves it
/// installed for other tests.
#[test]
fn ru_tab_labels_render_in_russian() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let labels = [
        rust_i18n::t!("general.tab_general").into_owned(),
        rust_i18n::t!("general.tab_hotkeys").into_owned(),
        rust_i18n::t!("general.tab_overlay").into_owned(),
    ];

    rust_i18n::set_locale("en");

    assert_eq!(
        labels,
        [
            "Общие".to_owned(),
            "Горячие клавиши".to_owned(),
            "Оверлей".to_owned()
        ],
        "tab headers must render the Russian catalog values under ru"
    );
    assert!(
        labels.iter().all(|label| !label.is_ascii()),
        "every ru tab label must contain Cyrillic, got {labels:?}"
    );
}

/// Compile dependency (rebuild hazard): cargo does NOT list `locales/app.yml`
/// in this crate's dep-info — rust-i18n reads the catalog during macro
/// expansion, and its build script's `rerun-if-changed` scope is the rust-i18n
/// crate's own directory — so editing a translation must still recompile via
/// the crate-level `I18N_CATALOG_SOURCE` (`include_str!`) edge. This test pins
/// that edge: the const exists, starts with the `_version: 2` line, and equals
/// the on-disk catalog byte-for-byte, so the embedded copy cannot drift from
/// the file the macro reads.
#[test]
fn catalog_source_is_embedded_as_a_compile_dependency() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("locales")
        .join("app.yml");
    let on_disk = std::fs::read(&path)
        .unwrap_or_else(|err| panic!("{} must exist and be readable: {err}", path.display()));

    assert!(
        I18N_CATALOG_SOURCE.starts_with("_version: 2"),
        "I18N_CATALOG_SOURCE must start with the `_version: 2` line of locales/app.yml"
    );
    assert_eq!(
        I18N_CATALOG_SOURCE.as_bytes(),
        on_disk.as_slice(),
        "the embedded catalog must equal locales/app.yml byte-for-byte"
    );
}

/// Interpolated texts substitute their placeholders under `ru`: the toast
/// keeps the Russian prefix plus the file name, and the duplicate error
/// carries both arguments. `ru` is restored BEFORE the assertions.
#[test]
fn ru_interpolated_toast_substitutes_placeholders() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let saved = rust_i18n::t!("toast.saved", file_name = "shot.png").into_owned();
    let duplicate =
        rust_i18n::t!("hotkey.duplicate_error", row = "Окно", combo = "Ctrl+P").into_owned();

    rust_i18n::set_locale("en");

    assert!(
        saved.starts_with("Сохранено"),
        "ru toast.saved must start with the Russian prefix, got {saved}"
    );
    assert!(
        saved.ends_with("shot.png"),
        "ru toast.saved must end with the substituted file name, got {saved}"
    );
    assert!(
        duplicate.contains("Окно") && duplicate.contains("Ctrl+P"),
        "ru duplicate error must carry both substituted args, got {duplicate}"
    );
}

/// Completeness at the RENDER level (Stage 4): every shipped key, called
/// through `t!` under each of the six locales, must produce a real localized
/// string — non-empty, never the key itself, and never a dotted-key echo
/// inside the text. Keys with `%{...}` placeholders are called without args;
/// the render is still real text. Each locale is restored to `en` BEFORE its
/// assertions so a failure never leaves the process-wide global mutated.
#[test]
fn all_six_locales_render_without_fallback_to_key_text() {
    let _guard = locale_guard();
    let entries = catalog_entries(&catalog_text());
    assert_eq!(
        entries.len(),
        76,
        "locales/app.yml must declare exactly 76 shipped keys"
    );

    for locale in SUPPORTED_LOCALES {
        rust_i18n::set_locale(locale);
        let rendered: Vec<String> = entries
            .iter()
            .map(|(key, _)| rust_i18n::t!(key.as_str()).into_owned())
            .collect();
        rust_i18n::set_locale("en");

        for ((key, _), text) in entries.iter().zip(&rendered) {
            assert!(
                !text.is_empty(),
                "key `{key}` must render a non-empty string under {locale}"
            );
            assert_ne!(
                text.as_str(),
                key.as_str(),
                "key `{key}` must resolve under {locale}, not echo itself"
            );
            assert!(
                !text.contains(&format!(".{key}")),
                "key `{key}` must not fall back to a dotted-key path inside the text under {locale}"
            );
        }
    }
}

/// Under each Stage 4 locale the tab headers render that locale's catalog
/// values (spec §7 dropdown). `en` is restored BEFORE each assertion.
#[test]
fn zh_cn_es_ja_ko_tab_labels_render_locally() {
    let _guard = locale_guard();
    let expected: [(&str, [&str; 3]); 4] = [
        ("zh-CN", ["通用", "快捷键", "悬浮提示"]),
        ("es", ["Generales", "Atajos", "Superposición"]),
        ("ja", ["一般", "ショートキー", "オーバーレイ"]),
        ("ko", ["일반", "단축키", "오버레이"]),
    ];

    for (locale, labels) in expected {
        rust_i18n::set_locale(locale);
        let rendered = [
            rust_i18n::t!("general.tab_general").into_owned(),
            rust_i18n::t!("general.tab_hotkeys").into_owned(),
            rust_i18n::t!("general.tab_overlay").into_owned(),
        ];
        rust_i18n::set_locale("en");

        assert_eq!(
            rendered,
            labels.map(str::to_owned),
            "tab headers must render the {locale} catalog values under {locale}"
        );
    }
}

/// Interpolated texts substitute their placeholders under every non-English
/// locale: the save toast keeps its localized prefix plus the file name, and
/// the collision toast carries both arguments. `en` is restored BEFORE each
/// assertion.
#[test]
fn interpolated_keys_substitute_placeholders_in_every_locale() {
    let _guard = locale_guard();
    for locale in SUPPORTED_LOCALES.iter().skip(1) {
        rust_i18n::set_locale(locale);
        let saved = rust_i18n::t!("toast.saved", file_name = "shot.png").into_owned();
        let collisions =
            rust_i18n::t!("toast.too_many_collisions", path = "/tmp/x", limit = 1000).into_owned();
        rust_i18n::set_locale("en");

        assert!(
            saved.ends_with("shot.png"),
            "{locale} toast.saved must end with the substituted file name, got {saved}"
        );
        assert_ne!(
            saved, "Saved: shot.png",
            "{locale} toast.saved must not render the English wording, got {saved}"
        );
        assert!(
            collisions.contains("/tmp/x") && collisions.contains("1000"),
            "{locale} too_many_collisions must carry both substituted args, got {collisions}"
        );
    }
}
