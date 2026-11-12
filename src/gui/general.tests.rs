use super::*;

use std::sync::MutexGuard;

/// Take the crate-wide locale lock (`crate::LOCALE_LOCK`): the locale and its
/// cache version are process-global, so locale-touching tests in every file
/// serialize on this one lock, not on a per-file one. A poisoned lock (a panic
/// in another locale test) is recovered rather than unwrapped, like every
/// other mailbox in this crate.
fn locale_guard() -> MutexGuard<'static, ()> {
    crate::LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Path as a `/`-separated string, so a fixture compares identically on
/// linux and windows-gnu (`join` inserts the host separator).
fn normalised(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Scratch directory under `./.tmp/` (repo root, git-ignored), unique per
/// tag, removed on drop — the same relative-path approach as the config
/// module tests (no tempdir dependency, identical on both targets).
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(tag: &str) -> Self {
        let path = PathBuf::from("./.tmp").join(format!("gui-test-{tag}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir must be creatable");
        Self { path }
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn format_labels_match_config_names() {
    assert_eq!(format_label(Format::Png), "png");
    assert_eq!(format_label(Format::Jpeg), "jpg");
    assert_eq!(format_label(Format::Webp), "webp");
}

#[test]
fn browse_start_dir_existing_path_used() {
    let dir = ScratchDir::new("browse-existing");

    assert_eq!(
        browse_start_dir(&dir.path, Some(Path::new("fixture-home"))),
        dir.path
    );
}

#[test]
fn browse_start_dir_falls_back_to_pictures() {
    let dir = ScratchDir::new("browse-missing");
    let missing = dir.path.join("no-such-folder");

    assert!(!missing.exists(), "the fixture path must not exist on disk");
    assert_eq!(
        normalised(&browse_start_dir(&missing, Some(Path::new("fixture-home")))),
        "fixture-home/Pictures"
    );
}

#[test]
fn browse_start_dir_without_home_is_relative_pictures() {
    let missing = PathBuf::from("./.tmp/gui-test-browse-no-home/absent");

    assert!(!missing.exists(), "the fixture path must not exist on disk");
    assert_eq!(normalised(&browse_start_dir(&missing, None)), "Pictures");
}

// ---------- placeholder reference (General tab accordion, Stage 6) ----------

#[test]
fn placeholder_reference_lines_are_derived_from_the_token_table() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The rows ARE `template::TOKENS` — one per entry, token spelled with the
    // `%`, description copied verbatim. Descriptions are catalog lookups
    // (`general.placeholder_desc_*`), so under a pinned `en` this equality
    // pins catalog == TOKENS byte-for-byte; a UI-side copy would drift.
    let lines = placeholder_reference_lines();
    assert_eq!(lines.len(), crate::template::TOKENS.len());
    for (line, (name, description)) in lines.iter().zip(crate::template::TOKENS) {
        assert_eq!(line.0, format!("%{name}"), "row must start with its token");
        assert_eq!(
            line.1, *description,
            "row text must come from the token table verbatim"
        );
    }
}

#[test]
fn placeholder_reference_lists_window_title_and_extension() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let lines = placeholder_reference_lines();
    let title_row = lines
        .iter()
        .find(|(token, _)| token == "%t")
        .expect("window title token must be listed");
    assert!(
        title_row.1.contains("title"),
        "%t row must explain the window title, got \"{}\"",
        title_row.1
    );
    let ext_row = lines
        .iter()
        .find(|(token, _)| token == "%ext")
        .expect("extension token must be listed");
    assert!(
        ext_row.1.contains("png"),
        "%ext row must show a real extension, got \"{}\"",
        ext_row.1
    );
}

// ---------- language dropdown (General tab, Stage 2b, spec §2) ----------

#[test]
fn language_options_are_the_six_supported_locales_with_native_names() {
    // The dropdown order IS this table: config code + NATIVE endonym.
    // Endonyms are never localized ("Русский" stays Russian in every
    // locale), so they are constants, not catalog keys. `en` is first
    // because it is the spec §7 default.
    assert_eq!(
        LANGUAGE_OPTIONS,
        [
            ("en", "English"),
            ("ru", "Русский"),
            ("zh-CN", "简体中文"),
            ("es", "Español"),
            ("ja", "日本語"),
            ("ko", "한국어"),
        ]
    );
    assert_eq!(LANGUAGE_OPTIONS[0].0, "en", "the default locale is first");
    // Every dropdown code must survive `config::language_code` verbatim —
    // the table and the normalization rule (src/config.rs) must agree.
    for (code, _) in LANGUAGE_OPTIONS {
        assert_eq!(
            crate::config::language_code(code),
            code,
            "dropdown code {code} must survive config normalization"
        );
    }
}

#[test]
fn language_dropdown_default_shows_english() {
    // A default draft selects the first option's native name: the selected
    // text of the ComboBox resolves through the option table.
    assert_eq!(
        native_name(&Settings::default().general.language),
        "English"
    );
    assert_eq!(
        LANGUAGE_OPTIONS[0].1,
        native_name(&Settings::default().general.language),
        "the selected text of a default draft is the first dropdown entry"
    );
}

#[test]
fn locale_code_of_draft_matches_config_normalization() {
    // `locale_code_of` is the Apply-time wrapper over `config::language_code`:
    // the same lenient EXACT-match rule as on load (spec §7), so the dropdown
    // value and a hand-edited config switch identically.
    let mut draft = Settings::default();
    draft.general.language = "ru".to_owned();
    assert_eq!(locale_code_of(&draft), "ru");
    draft.general.language = "de".to_owned();
    assert_eq!(
        locale_code_of(&draft),
        "en",
        "an unknown code normalizes to en"
    );
    draft.general.language = String::new();
    assert_eq!(
        locale_code_of(&draft),
        "en",
        "an empty code normalizes to en"
    );
}

/// The placeholder reference keeps token notation (`%Y`, `%t`, `%ext`) in
/// every locale — the token column is config notation, while the
/// description column follows the UI language. `ru` is restored BEFORE the
/// assertions.
#[test]
fn placeholder_reference_lines_follow_russian_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let lines = placeholder_reference_lines();

    rust_i18n::set_locale("en");

    assert_eq!(
        lines.len(),
        crate::template::TOKENS.len(),
        "one reference row per token in every locale"
    );
    for (line, (name, _)) in lines.iter().zip(crate::template::TOKENS) {
        assert_eq!(
            line.0,
            format!("%{name}"),
            "the token column must stay notation under ru"
        );
        assert!(
            !line.1.is_ascii(),
            "the description column must render Russian under ru, got \"{}\"",
            line.1
        );
    }

    // Notation inside a description survives verbatim: the format name in
    // `%ext` and the full-screen literal in `%t` are data, not prose.
    let ext = lines
        .iter()
        .find(|(token, _)| token == "%ext")
        .expect("extension token must be listed");
    assert!(
        ext.1.contains("png"),
        "%ext must keep the png notation under ru, got \"{}\"",
        ext.1
    );
    let title = lines
        .iter()
        .find(|(token, _)| token == "%t")
        .expect("title token must be listed");
    assert!(
        title.1.contains("fullscreen"),
        "%t must keep the fullscreen literal under ru, got \"{}\"",
        title.1
    );
}

/// Stage 4: under each new locale the description column follows the UI
/// language — it must differ from the pinned `en` value (a ja description
/// can contain ASCII digits, so the comparison is against the en value, not
/// `is_ascii`) — while the token column stays notation (`%Y`, `%t`, `%ext`).
/// zh-CN and ko descriptions must be non-ASCII. `en` descriptions are
/// captured first; every locale is restored to `en` BEFORE its assertions.
#[test]
fn placeholder_reference_lines_render_in_each_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");
    let en_lines = placeholder_reference_lines();

    for locale in ["zh-CN", "es", "ja", "ko"] {
        rust_i18n::set_locale(locale);
        let lines = placeholder_reference_lines();
        rust_i18n::set_locale("en");

        assert_eq!(
            lines.len(),
            crate::template::TOKENS.len(),
            "one reference row per token under {locale}"
        );
        for (index, (line, (name, _))) in lines.iter().zip(crate::template::TOKENS).enumerate() {
            assert_eq!(
                line.0,
                format!("%{name}"),
                "the token column must stay notation under {locale}"
            );
            assert_ne!(
                line.1, en_lines[index].1,
                "the description column must follow the UI language under {locale}, got \"{}\"",
                line.1
            );
        }
        if locale == "zh-CN" || locale == "ko" {
            assert!(
                lines.iter().all(|(_, description)| !description.is_ascii()),
                "every {locale} description must contain non-ASCII text, got {lines:?}"
            );
        }

        // Notation inside a description survives verbatim in every locale.
        let ext = lines
            .iter()
            .find(|(token, _)| token == "%ext")
            .expect("extension token must be listed");
        assert!(
            ext.1.contains("png"),
            "%ext must keep the png notation under {locale}, got \"{}\"",
            ext.1
        );
        let title = lines
            .iter()
            .find(|(token, _)| token == "%t")
            .expect("title token must be listed");
        assert!(
            title.1.contains("fullscreen"),
            "%t must keep the fullscreen literal under {locale}, got \"{}\"",
            title.1
        );
    }
}
