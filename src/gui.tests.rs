use super::*;

use crate::config::Controls;
use global_hotkey::hotkey::Code;
use global_hotkey::hotkey::Modifiers;
use std::sync::Mutex;
use std::sync::MutexGuard;

/// Serializes every test that reads or writes the process-wide locale —
/// `rust_i18n::locale()` is a process-wide global with a per-thread cache, so
/// locale-touching tests must not run in parallel (same pattern as
/// `i18n.tests.rs` / `capture.tests.rs`).
static LOCALE_LOCK: Mutex<()> = Mutex::new(());

/// Take the locale lock; a poisoned lock (a panic in another locale test) is
/// recovered rather than unwrapped, like every other mailbox in this crate.
fn locale_guard() -> MutexGuard<'static, ()> {
    LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Assert a rendered UI string goes through the catalog `key` AND the
/// catalog's `en` value equals the shipped English contract byte-for-byte.
/// Callers must hold `locale_guard()` and pin `"en"` first — this helper
/// calls `t!` itself. Tests that never leave `en` need no further restore:
/// the entry `set_locale("en")` pins this thread's cache for the whole test.
fn assert_en_catalog(key: &str, rendered: &str, contract: &str) {
    assert_eq!(
        rendered,
        rust_i18n::t!(key).as_ref(),
        "the rendered text must come from catalog key `{key}`"
    );
    assert_eq!(
        rust_i18n::t!(key).as_ref(),
        contract,
        "catalog `en` value for `{key}` must equal the shipped English wording"
    );
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

fn default_controls() -> Controls {
    Settings::default().controls
}

#[test]
fn supported_key_name_maps_egui_keys_to_config_names() {
    assert_eq!(
        supported_key_name(egui::Key::PrintScreen),
        Some("PrintScreen")
    );
    assert_eq!(supported_key_name(egui::Key::A), Some("A"));
    assert_eq!(supported_key_name(egui::Key::Num0), Some("0"));
    assert_eq!(supported_key_name(egui::Key::F1), Some("F1"));
    assert_eq!(supported_key_name(egui::Key::F12), Some("F12"));
    assert_eq!(supported_key_name(egui::Key::F13), Some("F13"));
    assert_eq!(supported_key_name(egui::Key::F24), Some("F24"));
    // egui's names differ from the crate/config names — mapped explicitly.
    assert_eq!(supported_key_name(egui::Key::ArrowDown), Some("ArrowDown"));
    assert_eq!(supported_key_name(egui::Key::ArrowLeft), Some("ArrowLeft"));
    assert_eq!(
        supported_key_name(egui::Key::ArrowRight),
        Some("ArrowRight")
    );
    assert_eq!(supported_key_name(egui::Key::ArrowUp), Some("ArrowUp"));
    assert_eq!(
        supported_key_name(egui::Key::OpenBracket),
        Some("BracketLeft")
    );
    assert_eq!(
        supported_key_name(egui::Key::CloseBracket),
        Some("BracketRight")
    );
    assert_eq!(supported_key_name(egui::Key::Backtick), Some("Backquote"));
    assert_eq!(supported_key_name(egui::Key::Equals), Some("Equal"));
    // Shift+`=` arrives as the logical `+`; the physical code is `Equal`.
    assert_eq!(supported_key_name(egui::Key::Plus), Some("Equal"));
    assert_eq!(supported_key_name(egui::Key::Menu), Some("ContextMenu"));
    assert_eq!(supported_key_name(egui::Key::Space), Some("Space"));
    assert_eq!(supported_key_name(egui::Key::Enter), Some("Enter"));
    assert_eq!(supported_key_name(egui::Key::Tab), Some("Tab"));
    assert_eq!(supported_key_name(egui::Key::Insert), Some("Insert"));
    assert_eq!(supported_key_name(egui::Key::Delete), Some("Delete"));
    assert_eq!(supported_key_name(egui::Key::Home), Some("Home"));
    assert_eq!(supported_key_name(egui::Key::End), Some("End"));
    assert_eq!(supported_key_name(egui::Key::PageUp), Some("PageUp"));
    assert_eq!(supported_key_name(egui::Key::PageDown), Some("PageDown"));
    assert_eq!(supported_key_name(egui::Key::Comma), Some("Comma"));
    assert_eq!(
        supported_key_name(egui::Key::MediaPlayPause),
        Some("MediaPlayPause")
    );
    assert_eq!(
        supported_key_name(egui::Key::AudioVolumeMute),
        Some("AudioVolumeMute")
    );
    assert_eq!(
        supported_key_name(egui::Key::BrowserHome),
        Some("BrowserHome")
    );
    assert_eq!(supported_key_name(egui::Key::Pause), Some("Pause"));
}

#[test]
fn supported_key_name_rejects_unsupported_keys() {
    // Reserved for the capture UI (§4.4): they must never become a binding.
    assert_eq!(supported_key_name(egui::Key::Escape), None);
    assert_eq!(supported_key_name(egui::Key::Backspace), None);
    // Modifier keys cannot be the main key of a Windows hotkey.
    assert_eq!(supported_key_name(egui::Key::ShiftLeft), None);
    assert_eq!(supported_key_name(egui::Key::ControlRight), None);
    assert_eq!(supported_key_name(egui::Key::AltLeft), None);
    assert_eq!(supported_key_name(egui::Key::SuperLeft), None);
    // Bindable but deliberately excluded: a bare registration would swallow
    // the lock toggle system-wide.
    assert_eq!(supported_key_name(egui::Key::CapsLock), None);
    assert_eq!(supported_key_name(egui::Key::NumLock), None);
    // `ScrollLock` IS supported (Stage 14): it is a Windows bind-probe key, so
    // the row must render the name the probe recorded.
    assert_eq!(
        supported_key_name(egui::Key::ScrollLock),
        Some("ScrollLock")
    );
    // egui delivers them, `global_hotkey::key_to_vk` cannot register them.
    assert_eq!(supported_key_name(egui::Key::F25), None);
    assert_eq!(supported_key_name(egui::Key::F35), None);
    assert_eq!(supported_key_name(egui::Key::Fn), None);
    assert_eq!(supported_key_name(egui::Key::Eject), None);
    assert_eq!(supported_key_name(egui::Key::Power), None);
    assert_eq!(supported_key_name(egui::Key::Clear), None);
    assert_eq!(supported_key_name(egui::Key::Copy), None);
    assert_eq!(supported_key_name(egui::Key::Cut), None);
    assert_eq!(supported_key_name(egui::Key::Paste), None);
    // Logical characters with no physical code to register.
    assert_eq!(supported_key_name(egui::Key::Colon), None);
    assert_eq!(supported_key_name(egui::Key::Pipe), None);
    assert_eq!(supported_key_name(egui::Key::Questionmark), None);
    assert_eq!(supported_key_name(egui::Key::Exclamationmark), None);
    assert_eq!(supported_key_name(egui::Key::OpenCurlyBracket), None);
    assert_eq!(supported_key_name(egui::Key::CloseCurlyBracket), None);
}

#[test]
fn every_capturable_egui_name_parses_as_a_binding() {
    // Drift check in both directions: every name the UI may write comes from a
    // key egui actually delivers, and every delivered key maps to a name
    // `bindings::parse_controls` accepts (CAPTURABLE ⊆ BINDABLE).
    for name in SUPPORTED_KEY_NAMES {
        assert!(
            egui::Key::ALL
                .iter()
                .any(|key| supported_key_name(*key) == Some(name)),
            "config name {name} must come from a capturable egui key"
        );
    }
    for key in egui::Key::ALL {
        let Some(name) = supported_key_name(*key) else {
            continue;
        };
        assert!(
            SUPPORTED_KEY_NAMES.contains(&name),
            "egui key {} maps to {name}, outside SUPPORTED_KEY_NAMES",
            key.name()
        );
        let mut controls = default_controls();
        controls.full_screen = binding_from_press(name, mods_from_press(false, false, false), None);
        assert!(
            crate::bindings::parse_controls(&controls).is_ok(),
            "captured name {name} must parse via bindings::parse_key"
        );
    }
}

#[test]
fn supported_key_names_all_parse_as_bindings() {
    for name in SUPPORTED_KEY_NAMES {
        let binding = binding_from_press(name, mods_from_press(true, false, false), None);
        let mut controls = default_controls();
        controls.full_screen = binding;
        assert!(
            crate::bindings::parse_controls(&controls).is_ok(),
            "dropdown name {name} must parse via bindings::parse_key"
        );
    }
}

#[test]
fn binding_label_alt_printscreen_shows_modifier_then_key() {
    let mods = vec!["Alt".to_owned()];
    assert_eq!(binding_label("PrintScreen", &mods), "Alt+PrintScreen");
}

#[test]
fn binding_label_ctrl_shift_s_orders_modifiers_canonically() {
    let mods = vec!["Shift".to_owned(), "Ctrl".to_owned()];
    assert_eq!(binding_label("S", &mods), "Ctrl+Shift+S");
}

#[test]
fn binding_label_bare_key_has_no_modifier_prefix() {
    assert_eq!(binding_label("PrintScreen", &[]), "PrintScreen");
}

#[test]
fn binding_label_case_insensitive_mods_normalize_to_canonical_names() {
    let mods: Vec<String> = vec!["shift".to_owned(), "CTRL".to_owned(), "alt".to_owned()];
    assert_eq!(binding_label("S", &mods), "Alt+Ctrl+Shift+S");
}

#[test]
fn binding_label_unknown_mod_is_skipped() {
    let mods: Vec<String> = vec!["Hyper".to_owned(), "alt".to_owned()];
    assert_eq!(binding_label("S", &mods), "Alt+S");
}

#[test]
fn binding_label_duplicate_mods_appear_once() {
    let mods: Vec<String> = vec!["alt".to_owned(), "Alt".to_owned()];
    assert_eq!(binding_label("PrintScreen", &mods), "Alt+PrintScreen");
}

#[test]
fn binding_label_win_mod_is_shown_last() {
    let mods: Vec<String> = vec!["Win".to_owned(), "Alt".to_owned()];
    assert_eq!(binding_label("S", &mods), "Alt+Win+S");
}

#[test]
fn binding_label_text_splits_back_into_a_parseable_binding() {
    // The label reads like §4.1 notation: splitting it back into the
    // config `key` + `mods` fields must yield a binding that parses.
    let mut controls = default_controls();
    controls.window.key = "S".to_owned();
    controls.window.mods = vec!["Ctrl".to_owned(), "Alt".to_owned()];

    let label = binding_label(&controls.window.key, &controls.window.mods);
    assert_eq!(label, "Alt+Ctrl+S");

    let mut parts = label.split('+');
    controls.window.key = parts.next_back().expect("key part").to_owned();
    controls.window.mods = parts.map(|part| part.to_owned()).collect();

    let parsed = crate::bindings::parse_controls(&controls).expect("label must parse back");
    assert!(
        parsed
            .iter()
            .any(|parsed| parsed.action == Action::Window && parsed.active)
    );
}

#[test]
fn mods_from_press_keeps_alt_ctrl_shift_in_canonical_order() {
    let mods = mods_from_press(true, true, true);
    assert_eq!(
        mods,
        vec!["Alt".to_owned(), "Ctrl".to_owned(), "Shift".to_owned()]
    );
}

#[test]
fn mods_from_press_never_emits_win() {
    // winit never reports Super as a modifier (spec §4.4 limitation), so a
    // press can never produce a Win binding.
    assert!(!mods_from_press(true, false, false).contains(&"Win".to_owned()));
    assert!(mods_from_press(false, false, false).is_empty());
}

#[test]
fn binding_from_press_replaces_key_and_mods_and_keeps_pad() {
    let binding = binding_from_press("S", vec!["Ctrl".to_owned()], Some("pad-a".to_owned()));
    assert_eq!(binding.key, "S");
    assert_eq!(binding.mods, vec!["Ctrl".to_owned()]);
    assert_eq!(binding.pad, Some("pad-a".to_owned()));
}

#[test]
fn probe_binding_from_press_replaces_key_and_mods_and_keeps_pad() {
    // A Windows bind-probe press commits with the SAME field split as an egui
    // press (Stage 14, §4.1): keyboard fields replace, the pad slot stays.
    let binding = Binding {
        key: "PrintScreen".to_owned(),
        mods: vec!["Alt".to_owned()],
        pad: Some("North".to_owned()),
    };
    let press = ProbePress {
        code: Code::ScrollLock,
        mods: Modifiers::CONTROL | Modifiers::SHIFT,
    };

    let committed = probe_binding_from_press(&binding, &press).expect("ScrollLock is bindable");

    assert_eq!(committed.key, "ScrollLock");
    assert_eq!(committed.mods, vec!["Ctrl".to_owned(), "Shift".to_owned()]);
    assert_eq!(committed.pad, Some("North".to_owned()));
    assert_eq!(
        binding.pad,
        Some("North".to_owned()),
        "the helper must not mutate the row"
    );
}

#[test]
fn probe_binding_from_press_orders_modifiers_like_binding_label() {
    // The notation comes from `mods_from_press`, so a probe commit renders in the
    // canonical order — the same label an egui press of the same combo produces.
    let binding = Binding {
        key: "A".to_owned(),
        mods: Vec::new(),
        pad: None,
    };
    let press = ProbePress {
        code: Code::PrintScreen,
        mods: Modifiers::SHIFT | Modifiers::ALT,
    };

    let committed = probe_binding_from_press(&binding, &press).expect("PrintScreen is bindable");

    assert_eq!(committed.key, "PrintScreen");
    assert_eq!(committed.mods, vec!["Alt".to_owned(), "Shift".to_owned()]);
    assert_eq!(
        binding_label(&committed.key, &committed.mods),
        "Alt+Shift+PrintScreen"
    );
}

#[test]
fn probe_binding_from_press_returns_none_for_an_unbindable_probe_key() {
    // `Fn` has no Windows virtual key, so a probe press naming it must produce NO
    // binding — the caller leaves the row untouched rather than writing a name
    // the parser would reject.
    let binding = Binding {
        key: "S".to_owned(),
        mods: Vec::new(),
        pad: None,
    };

    assert_eq!(
        probe_binding_from_press(
            &binding,
            &ProbePress {
                code: Code::Fn,
                mods: Modifiers::empty(),
            }
        ),
        None
    );
    assert_eq!(
        binding.key, "S",
        "an unsupported probe key must not rewrite the row"
    );
}

#[test]
fn probe_binding_from_press_ignores_a_meta_modifier() {
    // Win is not bindable by press (§4.4 limitation); a probe never registers
    // META, and if one ever did, the committed notation would drop it — exactly
    // as `mods_from_press` drops it for an egui press.
    let binding = Binding {
        key: "S".to_owned(),
        mods: Vec::new(),
        pad: None,
    };
    let press = ProbePress {
        code: Code::Pause,
        mods: Modifiers::META | Modifiers::ALT,
    };

    let committed = probe_binding_from_press(&binding, &press).expect("Pause is bindable");

    assert_eq!(committed.key, "Pause");
    assert_eq!(committed.mods, vec!["Alt".to_owned()]);
}

#[test]
fn clear_key_binding_removes_key_and_mods_but_keeps_pad() {
    let mut binding = Binding {
        key: "S".to_owned(),
        mods: vec!["Ctrl".to_owned(), "Alt".to_owned()],
        pad: Some("North".to_owned()),
    };

    let cleared = clear_key_binding(&mut binding);

    assert_eq!(cleared.key, "", "a cleared binding has no keyboard key");
    assert!(
        cleared.mods.is_empty(),
        "a cleared binding has no modifiers left"
    );
    assert_eq!(
        cleared.pad,
        Some("North".to_owned()),
        "Backspace must leave the pad slot untouched (§4.1)"
    );
    assert_eq!(
        binding, cleared,
        "the row must hold the cleared value, not the old one"
    );
}

#[test]
fn clear_key_binding_leaves_an_unbound_key_the_parser_rejects() {
    // The empty `key` is the unbound marker: §4.1 notation has no empty key,
    // so a draft with a cleared row must never reach `config::save` — that is
    // what `draft_savable` blocks Apply/OK for.
    let mut controls = default_controls();
    clear_key_binding(&mut controls.full_screen);
    assert!(
        crate::bindings::parse_controls(&controls).is_err(),
        "a cleared key must not parse as a hotkey combo"
    );
}

#[test]
fn binding_from_press_after_clear_replaces_key_and_mods_and_keeps_pad() {
    // Backspace clears the keyboard half; a later press rebinds exactly the
    // keyboard fields and still keeps the pad slot (§4.1 field split).
    let mut binding = Binding {
        key: "PrintScreen".to_owned(),
        mods: vec!["Alt".to_owned()],
        pad: Some("Guide".to_owned()),
    };
    clear_key_binding(&mut binding);

    let rebound = binding_from_press(
        "F5",
        mods_from_press(false, true, false),
        binding.pad.clone(),
    );
    assert_eq!(rebound.key, "F5");
    assert_eq!(rebound.mods, vec!["Ctrl".to_owned()]);
    assert_eq!(rebound.pad, Some("Guide".to_owned()));
}

#[test]
fn bind_button_label_while_capturing_shows_the_press_prompt() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The capture prompt is catalog-driven (`hotkey.capture_button_label`);
    // matching its en value proves the branch AND the wording.
    assert_en_catalog(
        "hotkey.capture_button_label",
        &bind_button_label(true, "PrintScreen", &[]),
        "Press keys…",
    );
}

#[test]
fn bind_button_label_of_a_cleared_binding_shows_the_click_prompt() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let mut binding = Binding::default();
    clear_key_binding(&mut binding);

    // An unbound row must say it needs a press, not paint an empty button:
    // the prompt is the catalog `hotkey.unassigned` en value.
    assert_en_catalog(
        "hotkey.unassigned",
        &bind_button_label(false, &binding.key, &binding.mods),
        "unbound — click to bind",
    );
}

#[test]
fn bind_button_label_of_a_bound_row_shows_the_binding() {
    assert_eq!(
        bind_button_label(false, "S", &["Ctrl".to_owned()]),
        "Ctrl+S"
    );
}

#[test]
fn draft_savable_requires_a_key_on_every_action() {
    assert!(
        draft_savable(&Settings::default()),
        "the shipped defaults are savable"
    );

    let mut draft = Settings::default();
    clear_key_binding(&mut draft.controls.window);
    assert!(
        !draft_savable(&draft),
        "a cleared row must block saving (§4.1)"
    );

    let mut draft = Settings::default();
    clear_key_binding(&mut draft.controls.rect);
    assert!(
        !draft_savable(&draft),
        "a cleared reserved row must block saving too"
    );
}

#[test]
fn failed_registration_maps_to_warning_icon_with_reason_tooltip() {
    let failed = Registration::Failed {
        action: Action::Window,
        combo: "alt+PrintScreen".to_owned(),
        reason: "hot key already registered by another app".to_owned(),
    };

    let icon = icon_for_status(&failed).expect("warning icon");
    assert_eq!(icon.kind, IconKind::Warning);
    assert_eq!(icon.tooltip, "hot key already registered by another app");
}

#[test]
fn registered_registration_maps_to_no_icon() {
    let registered = Registration::Registered {
        action: Action::FullScreen,
        combo: "PrintScreen".to_owned(),
    };
    assert_eq!(icon_for_status(&registered), None);
}

#[test]
fn reserved_registration_maps_to_info_icon_with_scope_tooltip() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let reserved = Registration::Reserved {
        action: Action::Rect,
        combo: "ctrl+PrintScreen".to_owned(),
    };

    let icon = icon_for_status(&reserved).expect("info icon");
    assert_eq!(icon.kind, IconKind::Info);
    // Exact en wording (was a `contains` proxy): the tooltip is the catalog
    // `hotkey.reserved_tooltip` render, byte-for-byte.
    assert_en_catalog(
        "hotkey.reserved_tooltip",
        &icon.tooltip,
        "reserved — out of MVP scope",
    );
}

#[test]
fn icon_glyphs_are_ascii_because_egui_default_fonts_lack_symbols() {
    // Verified against the locked font cmaps: Ubuntu-Light and egui-icons
    // (the only bundled fonts with our eframe features) have no ⚠/ℹ, so
    // the icons must be plain ASCII letters, colored.
    for kind in [IconKind::Warning, IconKind::Info] {
        assert!(icon_glyph(kind).is_ascii());
        assert_eq!(icon_glyph(kind).chars().count(), 1);
    }
}

#[test]
fn is_dirty_detects_draft_edits() {
    let applied = Settings::default();
    let mut draft = applied.clone();

    assert!(!is_dirty(&draft, &applied));

    draft.general.save_path = "/tmp/other".to_owned();
    assert!(is_dirty(&draft, &applied));
}

#[test]
fn bottom_buttons_apply_and_ok_require_dirty_draft() {
    // Apply and OK both run `apply()` (§4.4), so they are meaningful only while
    // the draft differs from what is applied AND every row has a key.
    let dirty = bottom_buttons_enabled(true, true);
    assert!(
        dirty.apply_enabled,
        "Apply must be enabled with a dirty, savable draft"
    );
    assert!(
        dirty.ok_enabled,
        "OK must be enabled with a dirty, savable draft"
    );

    let clean = bottom_buttons_enabled(false, true);
    assert!(
        !clean.apply_enabled,
        "Apply must stay disabled with a clean draft"
    );
    assert!(
        !clean.ok_enabled,
        "OK must stay disabled with a clean draft"
    );
}

#[test]
fn bottom_buttons_save_disabled_while_a_binding_is_cleared() {
    // Backspace clears a row to the unbound marker (§4.4); saving it would
    // write a key `bindings::parse_key` rejects (§4.1), which costs the app
    // its hotkeys and pad triggers, so Apply/OK stay disabled until re-bound.
    let blocked = bottom_buttons_enabled(true, false);
    assert!(
        !blocked.apply_enabled,
        "Apply must be disabled for an unbound row"
    );
    assert!(
        !blocked.ok_enabled,
        "OK must be disabled for an unbound row"
    );
    assert!(
        blocked.cancel_enabled,
        "Cancel discards the draft, so it stays enabled"
    );
}

#[test]
fn bottom_buttons_cancel_always_enabled() {
    // Cancel saves nothing — it only discards the draft, so it is enabled in
    // every draft state (§4.4).
    assert!(bottom_buttons_enabled(true, true).cancel_enabled);
    assert!(bottom_buttons_enabled(false, true).cancel_enabled);
    assert!(bottom_buttons_enabled(true, false).cancel_enabled);
}

#[test]
fn bottom_bar_panel_id_is_a_stable_non_empty_salt() {
    // Stage 11 docks the row into a real `egui::Panel::bottom`. egui decides
    // panel placement at paint time, so docking is proven by the Xvfb
    // screenshots (`scripts/ui-verify.sh`), not by a unit test; the id salt is
    // the only seam a test can reach, and pinning it keeps the panel id from
    // drifting into an unnamed layout.
    assert_eq!(BOTTOM_BAR_PANEL_ID, "settings_bottom_bar");
    assert!(!BOTTOM_BAR_PANEL_ID.is_empty());
}

/// Hotkey-row fixture: one `Binding` exactly as a draft row would hold it.
fn binding(key: &str, mods: &[&str], pad: Option<&str>) -> Binding {
    Binding {
        key: key.to_owned(),
        mods: mods.iter().copied().map(str::to_owned).collect(),
        pad: pad.map(str::to_owned),
    }
}

#[test]
fn duplicate_bindings_detects_same_key_with_same_mods() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let mut draft = Settings::default();
    // Window loses its Alt modifier: both rows are bare `PrintScreen`.
    draft.controls.window = binding("PrintScreen", &[], None);
    let duplicates = duplicate_bindings(&draft);
    assert_eq!(duplicates.len(), 2, "one collision reports both rows");
    assert_eq!(duplicates[0].action, Action::FullScreen);
    assert_eq!(duplicates[0].other, Action::Window);
    assert_eq!(duplicates[0].kind, DuplicateKind::Keyboard);
    // The description IS the catalog render of `hotkey.duplicate_error` with
    // the other row's label and this row's combo; the en contract constant
    // pins the shipped English wording (the old `is_ascii` proxy is gone —
    // Cyrillic lands in Stage 3, so locale-guarded en equality is the check).
    assert_eq!(
        duplicates[0].description,
        rust_i18n::t!(
            "hotkey.duplicate_error",
            row = row_label(Action::Window),
            combo = "PrintScreen"
        )
        .into_owned(),
        "the tooltip must render through catalog hotkey.duplicate_error"
    );
    assert_eq!(
        duplicates[0].description, "Duplicates Window: PrintScreen",
        "catalog `en` value must match the shipped English contract"
    );
    assert_eq!(duplicates[1].action, Action::Window);
    assert_eq!(duplicates[1].other, Action::FullScreen);
    assert_eq!(
        duplicates[1].description, "Duplicates Full screen: PrintScreen",
        "the mirrored row must name the other action"
    );
}

#[test]
fn duplicate_bindings_normalizes_modifier_order_and_case() {
    let mut draft = Settings::default();
    // `Ctrl+Shift+S` and a reordered, re-cased twin with a doubled modifier
    // are the same combo: both normalize to key `s` + mods `ctrl`, `shift`.
    draft.controls.full_screen = binding("S", &["Ctrl", "Shift"], None);
    draft.controls.window = binding("s", &["SHIFT", "ctrl", "Ctrl"], None);
    let duplicates = duplicate_bindings(&draft);
    assert_eq!(
        duplicates.len(),
        2,
        "modifier order, case, and repeats must not hide a collision"
    );
    assert!(duplicates.iter().all(|d| d.kind == DuplicateKind::Keyboard));
}

#[test]
fn duplicate_bindings_ignores_unbound_rows() {
    let mut draft = Settings::default();
    // The empty-key marker (§4.4): unbound rows never collide, even together.
    draft.controls.full_screen.key = String::new();
    draft.controls.window = binding("", &[], None);
    assert!(
        duplicate_bindings(&draft).is_empty(),
        "two unbound rows are not a collision"
    );
    draft.controls.window = binding("PrintScreen", &[], None);
    assert!(
        duplicate_bindings(&draft).is_empty(),
        "an unbound row never collides with a bound one"
    );
}

#[test]
fn duplicate_bindings_ignores_reserved_rect_action() {
    let mut draft = Settings::default();
    // The reserved rect row (§4.1) never registers or fires, so identical
    // triggers on it are not a collision — keyboard or pad.
    draft.controls.rect = draft.controls.full_screen.clone();
    assert!(
        duplicate_bindings(&draft).is_empty(),
        "the reserved rect row never collides (keyboard)"
    );
    draft.controls.full_screen.pad = Some("Guide".to_owned());
    draft.controls.rect.pad = Some("Guide".to_owned());
    assert!(
        duplicate_bindings(&draft).is_empty(),
        "the reserved rect row never collides (pad)"
    );
}

#[test]
fn duplicate_bindings_detects_pad_duplicate_case_insensitively() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let mut draft = Settings::default();
    // Keyboard stays unique (§7 defaults); only the pad strings collide.
    draft.controls.full_screen.pad = Some("Guide".to_owned());
    draft.controls.window.pad = Some("guide".to_owned());
    let duplicates = duplicate_bindings(&draft);
    assert_eq!(duplicates.len(), 2, "one pad collision reports both rows");
    assert!(duplicates.iter().all(|d| d.kind == DuplicateKind::Pad));
    // Catalog-driven description (`hotkey.duplicate_pad_error`): the other
    // row's label plus this row's stored pad notation, en contract pinned.
    assert_eq!(
        duplicates[0].description,
        rust_i18n::t!(
            "hotkey.duplicate_pad_error",
            row = row_label(Action::Window),
            pad = "Guide"
        )
        .into_owned(),
        "the tooltip must render through catalog hotkey.duplicate_pad_error"
    );
    assert_eq!(
        duplicates[0].description, "Duplicates Window pad: Guide",
        "catalog `en` value must match the shipped English contract"
    );
    assert_eq!(
        duplicates[1].description, "Duplicates Full screen pad: guide",
        "the mirrored row keeps the notation case it stored"
    );
    // An empty pad string means "no pad trigger", never a collision.
    draft.controls.full_screen.pad = Some(String::new());
    draft.controls.window.pad = Some(String::new());
    assert!(
        duplicate_bindings(&draft).is_empty(),
        "empty pad strings never collide"
    );
}

#[test]
fn duplicate_bindings_allows_different_mods_or_keys() {
    assert!(
        duplicate_bindings(&Settings::default()).is_empty(),
        "the §7 default bindings are unique"
    );
    let mut draft = Settings::default();
    draft.controls.window = binding("A", &["Alt"], None);
    assert!(
        duplicate_bindings(&draft).is_empty(),
        "a different key is not a collision"
    );
    draft.controls.window = binding("PrintScreen", &["Shift"], None);
    assert!(
        duplicate_bindings(&draft).is_empty(),
        "a different modifier set is not a collision"
    );
}

#[test]
fn bottom_buttons_save_disabled_while_bindings_duplicate() {
    let mut draft = Settings::default();
    draft.controls.window = binding("PrintScreen", &[], None);
    assert!(is_dirty(&draft, &Settings::default()));
    assert!(!duplicate_bindings(&draft).is_empty());
    // The Stage-3 gate composes uniqueness into `savable`; Cancel stays free.
    let savable = draft_savable(&draft) && duplicate_bindings(&draft).is_empty();
    assert!(!savable, "a duplicated draft must not be savable");
    let blocked = bottom_buttons_enabled(true, savable);
    assert!(
        !blocked.apply_enabled,
        "Apply must be disabled for a duplicate"
    );
    assert!(!blocked.ok_enabled, "OK must be disabled for a duplicate");
    assert!(
        blocked.cancel_enabled,
        "Cancel stays enabled while a duplicate blocks saving"
    );
}

/// The keyboard warning of a row (§4.4, Stage 10) belongs to the bind button,
/// so it must be composed ONLY from keyboard signals: this row's keyboard
/// collision and this row's registration failure.
#[test]
fn keyboard_warning_names_registration_failure_for_that_action_only() {
    let draft = Settings::default();
    let reason = "HotKey already registered: HotKey { mods: Alt, key: PrintScreen }";

    // The row whose registration failed gets the reason as its tooltip…
    assert_eq!(
        keyboard_warning(&draft, Action::Window, Some(reason)),
        Some(reason.to_owned()),
        "a registration failure must warn its own row"
    );
    // …and the other rows stay silent: a warning never leaks across rows.
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, None),
        None,
        "a healthy row shows no keyboard warning"
    );
    assert_eq!(
        keyboard_warning(&draft, Action::Rect, None),
        None,
        "the reserved row shows no keyboard warning"
    );
}

#[test]
fn keyboard_warning_names_keyboard_duplicate() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let mut draft = Settings::default();
    // Both active rows are bare `PrintScreen` now — the keyboard collision.
    draft.controls.window = binding("PrintScreen", &[], None);
    // The tooltip names the other row and this row's combo — rendered
    // through catalog `hotkey.duplicate_error`, en contract pinned.
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, None),
        Some(
            rust_i18n::t!(
                "hotkey.duplicate_error",
                row = row_label(Action::Window),
                combo = "PrintScreen"
            )
            .into_owned()
        ),
        "the tooltip must render through catalog hotkey.duplicate_error"
    );
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, None),
        Some("Duplicates Window: PrintScreen".to_owned()),
        "catalog `en` value must match the shipped English contract"
    );
    assert_eq!(
        keyboard_warning(&draft, Action::Window, None),
        Some("Duplicates Full screen: PrintScreen".to_owned()),
        "both colliding rows warn their own bind button"
    );
    assert_eq!(
        keyboard_warning(&draft, Action::Rect, None),
        None,
        "the reserved row never collides (§4.1)"
    );
}

#[test]
fn keyboard_warning_prefers_a_duplicate_over_a_registration_failure() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let mut draft = Settings::default();
    draft.controls.window = binding("PrintScreen", &[], None);
    // A collision blocks Apply — that is the actionable state, so it wins over
    // the stale registration status of the last applied settings. The winning
    // tooltip is the catalog `hotkey.duplicate_error` render under `en`.
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, Some("already registered")),
        Some(
            rust_i18n::t!(
                "hotkey.duplicate_error",
                row = row_label(Action::Window),
                combo = "PrintScreen"
            )
            .into_owned()
        ),
        "a draft collision outranks a registration failure"
    );
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, Some("already registered")),
        Some("Duplicates Window: PrintScreen".to_owned()),
        "catalog `en` value must match the shipped English contract"
    );
}

#[test]
fn keyboard_warning_ignores_pad_duplicate() {
    let mut draft = Settings::default();
    // Only the pad strings collide; the keyboard half is untouched (§7 defaults).
    draft.controls.full_screen.pad = Some("XboxX".to_owned());
    draft.controls.window.pad = Some("XboxX".to_owned());
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, None),
        None,
        "a pad collision must not warn the bind button"
    );
    assert_eq!(
        keyboard_warning(&draft, Action::Window, None),
        None,
        "a pad collision must not warn the other bind button either"
    );
}

/// The pad warning of a row (§4.4, Stage 10) belongs to the pad field, so it
/// must be composed ONLY from pad signals.
#[test]
fn pad_warning_names_pad_duplicate_only_for_pad_rows() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let mut draft = Settings::default();
    // Keyboard stays unique (§7 defaults); only the pad notation collides.
    draft.controls.full_screen.pad = Some("XboxX".to_owned());
    draft.controls.window.pad = Some("xboxx".to_owned());
    // Catalog-driven tooltip (`hotkey.duplicate_pad_error`): the other row's
    // label plus this row's stored pad notation, en contract pinned.
    assert_eq!(
        pad_warning(&draft, Action::FullScreen),
        Some(
            rust_i18n::t!(
                "hotkey.duplicate_pad_error",
                row = row_label(Action::Window),
                pad = "XboxX"
            )
            .into_owned()
        ),
        "the tooltip must render through catalog hotkey.duplicate_pad_error"
    );
    assert_eq!(
        pad_warning(&draft, Action::FullScreen),
        Some("Duplicates Window pad: XboxX".to_owned()),
        "catalog `en` value must match the shipped English contract"
    );
    assert_eq!(
        pad_warning(&draft, Action::Window),
        Some("Duplicates Full screen pad: xboxx".to_owned()),
        "both colliding rows warn their own pad field"
    );
    // An unbound pad never collides, so the slot stays silent.
    draft.controls.full_screen.pad = None;
    assert_eq!(
        pad_warning(&draft, Action::FullScreen),
        None,
        "an unbound pad slot shows no warning"
    );
    // The reserved rect row never appears in `duplicates` (§4.1).
    draft.controls.rect.pad = Some("XboxX".to_owned());
    assert_eq!(
        pad_warning(&draft, Action::Rect),
        None,
        "the reserved row shows no pad warning"
    );
}

#[test]
fn pad_warning_ignores_keyboard_duplicate() {
    let mut draft = Settings::default();
    draft.controls.window = binding("PrintScreen", &[], None);
    assert_eq!(
        pad_warning(&draft, Action::FullScreen),
        None,
        "a keyboard collision must not warn the pad field"
    );
    assert_eq!(
        pad_warning(&draft, Action::Window),
        None,
        "a keyboard collision must not warn the other pad field either"
    );
}

#[test]
fn duplicate_save_tooltip_explains_why_saving_is_blocked() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Disabled Apply/OK must tell the user what to do: exact wording via
    // catalog `hotkey.duplicate_tooltip`, pinned byte-for-byte as the en
    // contract (spec §2, §4.4).
    assert_en_catalog(
        "hotkey.duplicate_tooltip",
        &duplicate_save_tooltip(),
        "Duplicate binding: one combo or pad button must trigger only one action — re-bind one of the rows",
    );
}

#[test]
fn cancel_draft_restores_applied_and_keeps_applied_unchanged() {
    let applied = Settings::default();
    let mut draft = applied.clone();
    draft.general.save_path = "/tmp/other".to_owned();

    let restored = cancel_draft(&draft, &applied);
    assert_eq!(restored, applied, "Cancel must return the applied settings");
    assert_eq!(
        applied,
        Settings::default(),
        "Cancel must not rewrite the applied settings"
    );
    assert_eq!(
        draft.general.save_path, "/tmp/other",
        "Cancel must not rewrite the draft argument"
    );

    // A clean Cancel yields the same settings too.
    assert_eq!(cancel_draft(&applied, &applied), applied);
}

#[test]
fn close_decision_plain_window_close_hides_to_tray() {
    // The X button is not an exit: the process stays resident (§4.5 — exit is
    // tray-menu-only) and the window only hides.
    assert_eq!(close_decision(false, false), CloseAction::HideToTray);
}

#[test]
fn close_decision_ok_or_cancel_button_closes_for_real() {
    // OK/Cancel are explicit decisions: the close request must not be
    // swallowed by the hide-to-tray branch (§4.4).
    assert_eq!(close_decision(false, true), CloseAction::Exit);
}

#[test]
fn close_decision_tray_exit_closes_for_real() {
    assert_eq!(close_decision(true, false), CloseAction::Exit);
    assert_eq!(close_decision(true, true), CloseAction::Exit);
}

#[test]
fn tab_labels_are_english() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Each tab header renders through its catalog key; the en value pins the
    // shipped English wording byte-for-byte.
    assert_en_catalog("general.tab_general", &Tab::General.label(), "General");
    assert_en_catalog("general.tab_hotkeys", &Tab::Hotkeys.label(), "Hotkeys");
    assert_en_catalog("general.tab_overlay", &Tab::Overlay.label(), "Overlay");
}

#[test]
fn row_labels_are_english_per_spec() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Row labels render through catalog `hotkey.row_*`; the en values pin the
    // shipped English wording byte-for-byte.
    assert_en_catalog(
        "hotkey.row_full_screen",
        &row_label(Action::FullScreen),
        "Full screen",
    );
    assert_en_catalog("hotkey.row_window", &row_label(Action::Window), "Window");
    assert_en_catalog("hotkey.row_rect", &row_label(Action::Rect), "Rect area");
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

#[test]
fn capture_transition_idle_click_pauses_hotkeys() {
    assert_eq!(
        capture_transition(false, CaptureInput::Click),
        Some(CaptureTransition::Pause)
    );
}

#[test]
fn capture_transition_reclick_while_capturing_restores_hotkeys() {
    assert_eq!(
        capture_transition(true, CaptureInput::Click),
        Some(CaptureTransition::Restore)
    );
}

#[test]
fn capture_transition_escape_and_commit_restore_only_while_capturing() {
    assert_eq!(
        capture_transition(true, CaptureInput::Escape),
        Some(CaptureTransition::Restore)
    );
    assert_eq!(
        capture_transition(true, CaptureInput::Commit),
        Some(CaptureTransition::Restore)
    );
    assert_eq!(capture_transition(false, CaptureInput::Escape), None);
    assert_eq!(capture_transition(false, CaptureInput::Commit), None);
}

#[test]
fn capture_transition_clear_ends_capture_and_restores_only_while_capturing() {
    // Backspace clears the binding and ENDS capture (§4.4), so the live
    // registrations must come back exactly like on Escape/Commit.
    assert_eq!(
        capture_transition(true, CaptureInput::Clear),
        Some(CaptureTransition::Restore)
    );
    assert_eq!(capture_transition(false, CaptureInput::Clear), None);
}

#[test]
fn bind_hint_mentions_escape_and_backspace() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // One English line at the start of the bind form, painted on every pass
    // (§4.4 v8: it is not gated by capture state). Exact wording comes from
    // catalog `hotkey.bind_hint`; the old `is_ascii` proxy (spec §2 i18n
    // note) is replaced by locale-guarded en equality, because Stage 3
    // brings Cyrillic into the catalog — ASCII no longer means "English".
    assert_en_catalog(
        "hotkey.bind_hint",
        &bind_hint(),
        "Esc to cancel, Backspace to clear",
    );
}

#[test]
fn unbound_save_tooltip_explains_why_saving_is_blocked() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Disabled Apply/OK must tell the user what to do (spec §2). Exact
    // wording via catalog `hotkey.bind_all_hint`; the en contract constant
    // keeps the Backspace affordance in the pinned text.
    assert_en_catalog(
        "hotkey.bind_all_hint",
        &unbound_save_tooltip(),
        "Bind a key on every row before saving — Backspace clears a binding",
    );
}

#[test]
fn pad_capture_toggle_idle_click_starts_capture_on_that_row() {
    assert_eq!(
        pad_capture_toggle(false, Action::FullScreen),
        Some(Action::FullScreen)
    );
}

#[test]
fn pad_capture_toggle_reclick_while_capturing_cancels() {
    assert_eq!(pad_capture_toggle(true, Action::FullScreen), None);
}

#[test]
fn pad_capture_target_records_raw_index_notation_on_capturing_row() {
    // On Linux there is no device-info label, so the notation falls back to the
    // raw index (§9); on Windows the same call records the device label name.
    assert_eq!(
        pad_capture_target(
            Some(Action::Window),
            PadDownEvent::Controller {
                device: 0,
                index: 7
            },
        ),
        Some((Action::Window, "unknown:7".to_owned()))
    );
}

#[test]
fn pad_capture_target_records_system_button_names() {
    assert_eq!(
        pad_capture_target(Some(Action::FullScreen), PadDownEvent::Guide),
        Some((Action::FullScreen, "Guide".to_owned()))
    );
    assert_eq!(
        pad_capture_target(Some(Action::FullScreen), PadDownEvent::Share),
        Some((Action::FullScreen, "Share".to_owned()))
    );
}

#[test]
fn pad_capture_target_without_capturing_row_drops_the_press() {
    assert_eq!(pad_capture_target(None, PadDownEvent::Guide), None);
}

#[test]
fn pad_field_label_without_gameinput_backend_is_disabled_placeholder() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Both capture states fall through to catalog `gamepad.windows_only_note`;
    // matching its en value proves the branch AND the wording.
    assert_en_catalog(
        "gamepad.windows_only_note",
        &pad_field_label(false, false, Some("South")),
        "(gamepad — Windows only)",
    );
    assert_en_catalog(
        "gamepad.windows_only_note",
        &pad_field_label(false, true, Some("South")),
        "(gamepad — Windows only)",
    );
}

#[test]
fn pad_field_label_capturing_shows_press_prompt() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The pad capture prompt is catalog-driven
    // (`gamepad.capture_button_label`); matching its en value proves the
    // branch AND the wording.
    assert_en_catalog(
        "gamepad.capture_button_label",
        &pad_field_label(true, true, Some("South")),
        "Press a button…",
    );
}

#[test]
fn pad_field_label_shows_notation_or_click_to_bind_prompt() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Stored notation is config data (§4.1), never localized text.
    assert_eq!(pad_field_label(true, false, Some("unknown:3")), "unknown:3");
    assert_eq!(pad_field_label(true, false, Some("Guide")), "Guide");
    // The unbound slot is catalog-driven (`hotkey.pad_label`).
    assert_en_catalog(
        "hotkey.pad_label",
        &pad_field_label(true, false, None),
        "pad — click to bind",
    );
}

#[test]
fn pad_field_label_shows_the_device_label_name() {
    // Stage 5: the stored notation IS the device-info label of the captured
    // press, so the field shows the human-readable name verbatim — the
    // transient beside-field hint (pad_hints) is gone.
    assert_eq!(pad_field_label(true, false, Some("XboxX")), "XboxX");
    assert_eq!(pad_field_label(true, false, Some("Select")), "Select");
}

#[test]
fn clear_pad_binding_removes_pad_but_keeps_key_and_mods() {
    // Mirror of `clear_key_binding`: Backspace in pad capture clears ONLY the
    // pad slot (§4.1 field split), leaving the keyboard half of the row alone.
    let mut binding = Binding {
        key: "S".to_owned(),
        mods: vec!["Ctrl".to_owned(), "Alt".to_owned()],
        pad: Some("XboxY".to_owned()),
    };

    let cleared = clear_pad_binding(&mut binding);

    assert_eq!(cleared.pad, None, "a cleared binding has no pad trigger");
    assert_eq!(
        cleared.key, "S",
        "pad Backspace must leave the keyboard key untouched (§4.1)"
    );
    assert_eq!(
        cleared.mods,
        vec!["Ctrl".to_owned(), "Alt".to_owned()],
        "pad Backspace must leave the modifiers untouched (§4.1)"
    );
    assert_eq!(
        binding, cleared,
        "the row must hold the cleared value, not the old one"
    );
}

#[test]
fn clear_pad_binding_on_unbound_pad_is_noop() {
    let mut binding = binding_from_press("PrintScreen", mods_from_press(true, false, false), None);
    let before = binding.clone();

    let cleared = clear_pad_binding(&mut binding);

    assert_eq!(cleared, before, "an already-empty pad slot changes nothing");
    let mut controls = default_controls();
    controls.full_screen = cleared;
    assert!(
        crate::bindings::parse_controls(&controls).is_ok(),
        "clearing a pad slot must not block saving — §4.1 needs a keyboard key only"
    );
}

#[test]
fn pad_capture_esc_cancels_without_changing_binding() {
    // Escape ENDS a pad capture and writes nothing to the row (§4.4, v2): the
    // bound pad and the keyboard half both stay exactly as they were.
    let mut binding = binding_from_press(
        "S",
        mods_from_press(false, true, false),
        Some("XboxY".to_owned()),
    );

    assert_eq!(
        pad_capture_action(&mut binding, egui::Key::Escape),
        PadCaptureChange::Cancel
    );
    assert_eq!(
        binding.pad,
        Some("XboxY".to_owned()),
        "Escape keeps the pad bound"
    );
    assert_eq!(binding.key, "S", "Escape keeps the keyboard key");
    assert_eq!(
        binding.mods,
        vec!["Ctrl".to_owned()],
        "Escape keeps the modifiers"
    );
}

#[test]
fn pad_capture_backspace_clears_pad_only() {
    // Backspace ENDS a pad capture and clears ONLY the pad slot (§4.4, v2).
    let mut binding = binding_from_press(
        "S",
        mods_from_press(false, true, false),
        Some("XboxY".to_owned()),
    );

    assert_eq!(
        pad_capture_action(&mut binding, egui::Key::Backspace),
        PadCaptureChange::Clear
    );
    assert_eq!(binding.pad, None, "Backspace clears the pad slot");
    assert_eq!(
        binding.key, "S",
        "the keyboard half stays — the mirror of the keyboard clear"
    );
    assert_eq!(binding.mods, vec!["Ctrl".to_owned()], "the modifiers stay");
}

#[test]
fn pad_capture_ignores_other_keys_and_leaves_the_row_untouched() {
    // A keyboard press never binds the pad field: pad capture waits for a pad
    // press, so any other key keeps capture open and mutates nothing.
    let mut binding = binding_from_press(
        "S",
        mods_from_press(false, false, false),
        Some("XboxY".to_owned()),
    );
    let before = binding.clone();

    assert_eq!(
        pad_capture_action(&mut binding, egui::Key::A),
        PadCaptureChange::Ignore
    );
    assert_eq!(
        binding, before,
        "a keyboard press must never rewrite the pad slot"
    );
}

#[test]
fn keyboard_capture_backspace_still_keeps_pad() {
    // Pins the asymmetry that Stage 2 shipped (§4.1): keyboard Backspace drops
    // key+mods and leaves the pad slot, exactly as pad Backspace (above) leaves
    // the keyboard fields.
    let mut binding = binding_from_press(
        "F5",
        mods_from_press(true, false, false),
        Some("Guide".to_owned()),
    );

    let cleared = clear_key_binding(&mut binding);

    assert_eq!(cleared.key, "", "keyboard Backspace clears the key");
    assert!(
        cleared.mods.is_empty(),
        "keyboard Backspace clears the modifiers"
    );
    assert_eq!(cleared.pad, Some("Guide".to_owned()), "the pad slot stays");
}

#[test]
fn clear_pad_binding_ends_the_pad_duplicate_warning() {
    // The cleared slot is `None`, so the collision in the draft disappears and
    // Apply becomes possible again (§4.4 save gate).
    let mut draft = Settings::default();
    draft.controls.full_screen.pad = Some("XboxX".to_owned());
    draft.controls.window.pad = Some("xboxx".to_owned());
    assert!(
        pad_warning(&draft, Action::FullScreen).is_some(),
        "fixture: rows collide"
    );

    clear_pad_binding(&mut draft.controls.window);

    assert!(
        pad_warning(&draft, Action::FullScreen).is_none(),
        "clearing one pad slot must end the pad collision"
    );
    assert!(
        draft_savable(&draft),
        "a cleared pad slot never blocks saving (§4.1 needs a key only)"
    );
}

#[test]
fn pad_capture_tooltip_mentions_escape_and_backspace() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The pad field states the same affordances as the shared hint line
    // (§4.4, v2), sourced through catalog `gamepad.capture_tooltip`: the
    // exact en wording names Esc and Backspace, so the earlier `contains`
    // checks are subsumed by the byte-for-byte contract.
    assert_en_catalog(
        "gamepad.capture_tooltip",
        &pad_capture_tooltip(),
        "Press a gamepad button to bind it — Esc to cancel, Backspace to clear; click again to cancel.",
    );
}

#[test]
fn warning_color_image_embedded_asset_decodes_128x128() {
    // egui 0.37 has no PNG loader, so the embedded triangle asset goes
    // through `icons::rgba` into a straight-alpha ColorImage.
    let image = warning_color_image().expect("embedded warning-icon asset must decode");
    assert_eq!(image.size, [128, 128]);
    assert_eq!(image.pixels.len(), 128 * 128);
}

#[test]
fn warning_color_image_embedded_asset_has_yellow_and_black_pixels() {
    // The warning triangle body is yellow and the outline/exclamation is
    // black, both fully opaque — the decoded image must carry them.
    let image = warning_color_image().expect("embedded warning-icon asset must decode");
    let has_yellow = image.pixels.iter().any(|pixel| {
        let [r, g, b, a] = pixel.to_srgba_unmultiplied();
        r > 200 && g > 140 && b < 120 && a > 200
    });
    let has_black = image.pixels.iter().any(|pixel| {
        let [r, g, b, a] = pixel.to_srgba_unmultiplied();
        r < 80 && g < 80 && b < 80 && a > 200
    });
    assert!(has_yellow, "warning icon must contain opaque yellow pixels");
    assert!(has_black, "warning icon must contain opaque black pixels");
}

#[test]
fn app_window_icon_embedded_asset_returns_icon_data_128x128() {
    let icon = app_window_icon().expect("embedded app-icon asset must decode");
    assert_eq!(icon.width, 128);
    assert_eq!(icon.height, 128);
    assert_eq!(icon.rgba.len(), 128 * 128 * 4);
}

#[test]
fn app_window_icon_embedded_asset_has_transparent_corners() {
    // Straight (unmultiplied) alpha: the embedded asset's corner pixels are
    // fully transparent, so the window icon keeps its rounded silhouette.
    let icon = app_window_icon().expect("embedded app-icon asset must decode");
    let stride = icon.width as usize * 4;
    let corners = [
        0,
        stride - 4,
        stride * (icon.height as usize - 1),
        icon.rgba.len() - 4,
    ];
    for corner in corners {
        assert_eq!(
            icon.rgba[corner + 3],
            0,
            "corner alpha must be 0 at {corner}"
        );
    }
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

/// The Overlay tab test-button gate (Stage 7): enabled exactly while the
/// "Show overlay toast" checkbox is checked. A direct `ToastHandle::show`
/// bypasses `capture::should_show`, so this GUI-side gate is the only thing
/// keeping the test chip honest to spec §4.3.
#[test]
fn overlay_test_button_enabled_only_when_overlay_toast_enabled() {
    assert!(overlay_test_button_enabled(true));
    assert!(!overlay_test_button_enabled(false));
}

/// The test chip text is the real save success wording (spec §4.3): built by
/// the very helper `capture::toast_from_outcome` uses for a real save, so the
/// button cannot drift from the `Saved: <file name>` format.
#[test]
fn overlay_test_message_matches_real_save_success_wording() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    assert_eq!(
        overlay_test_message(OVERLAY_TEST_FILENAME),
        rust_i18n::t!("toast.saved", file_name = OVERLAY_TEST_FILENAME).into_owned(),
        "the chip text must render through catalog toast.saved"
    );
    assert_eq!(
        overlay_test_message(OVERLAY_TEST_FILENAME),
        "Saved: test.png",
        "catalog `en` value must match the shipped English contract"
    );
    assert_eq!(
        overlay_test_message(OVERLAY_TEST_FILENAME),
        capture::saved_text(Path::new("/any/dir/test.png")),
        "the test chip must spell the success text exactly like a real save"
    );
}

/// Pure half of the "no file" promise (Stage 7): the test chip is a text chip
/// carrying the fixed honest name — no image bytes, no save path — so the
/// click path has nothing to write; the Xvfb driver proves `out/` stays empty
/// and the log never carries a real `Saved: <path>` capture line.
#[test]
fn overlay_test_does_not_write_a_file() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let chip = test_overlay_toast(&Overlay::default());
    assert!(chip.image.is_none(), "test chip must carry no image bytes");
    assert_eq!(
        chip.text,
        overlay_test_message(OVERLAY_TEST_FILENAME),
        "chip text must name the honest test file, never a real saved path"
    );
}

/// The chip reflects the CURRENT draft — corner, monitor selection, and the
/// clamped hold follow the same rules a real save applies to `config::Overlay`,
/// so the chip shows what the user just configured even before Apply.
#[test]
fn overlay_test_toast_mirrors_draft_position_duration_and_monitors() {
    let draft = Overlay {
        enabled: true,
        position: OverlayPosition::TopRight,
        duration_ms: 250,
        on_all_monitors: false,
    };
    let chip = test_overlay_toast(&draft);
    assert_eq!(chip.corner, Corner::TopRight);
    assert_eq!(chip.monitor, MonitorSel::Index(0), "primary is index 0");
    assert_eq!(chip.duration_ms, 250);
    assert!(matches!(chip.color, ToastColor::Success));

    let sticky_all = Overlay {
        enabled: true,
        position: OverlayPosition::BottomLeft,
        duration_ms: 0,
        on_all_monitors: true,
    };
    let chip = test_overlay_toast(&sticky_all);
    assert_eq!(chip.duration_ms, 0, "0 stays sticky, like a real chip");
    assert_eq!(chip.monitor, MonitorSel::All);
    assert_eq!(chip.corner, Corner::BottomLeft);

    let huge = Overlay {
        duration_ms: u64::MAX,
        ..Overlay::default()
    };
    assert_eq!(
        test_overlay_toast(&huge).duration_ms,
        u32::MAX,
        "an absurd duration clamps instead of wrapping, like capture's rule"
    );
}

/// Disabled-button tooltip wording (pinned, spec §4.4 English UI): the hint
/// names the checkbox the user must tick before the test chip is possible,
/// sourced through catalog `overlay.test_disabled_hint` under a pinned `en`.
#[test]
fn overlay_test_disabled_hint_names_the_overlay_checkbox() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    assert_en_catalog(
        "overlay.test_disabled_hint",
        &overlay_test_disabled_hint(),
        "Enable the overlay toast to test it",
    );
}

// Stage 13 (spec §4.1 v2): failed registrations are retried when the settings
// window GAINS focus, and a refused combo is reported in readable UI text.
#[test]
fn retry_decision_gaining_focus_with_a_failed_row_retries() {
    // First pass where the window is focused IS a transition (`None` = "no
    // pass yet"), which is exactly the startup case: `main` registers before
    // any window exists.
    assert!(should_retry_failed_registrations(
        None,
        Some(true),
        true,
        false
    ));
    assert!(should_retry_failed_registrations(
        Some(false),
        Some(true),
        true,
        false
    ));
    // Unknown focus is not "focused": no retry on a signal we cannot trust.
    assert!(!should_retry_failed_registrations(None, None, true, false));
}

#[test]
fn retry_decision_staying_or_losing_focus_never_retries() {
    // Once per transition, never per frame.
    assert!(!should_retry_failed_registrations(
        Some(true),
        Some(true),
        true,
        false
    ));
    assert!(!should_retry_failed_registrations(
        Some(true),
        Some(false),
        true,
        false
    ));
    assert!(!should_retry_failed_registrations(
        None,
        Some(false),
        true,
        false
    ));
}

#[test]
fn retry_decision_without_a_failed_row_skips_the_pass() {
    // A healthy report has nothing to retry — the controller is not called.
    assert!(!should_retry_failed_registrations(
        None,
        Some(true),
        false,
        false
    ));
    assert!(!should_retry_failed_registrations(
        Some(false),
        Some(true),
        false,
        false
    ));
}

#[test]
fn retry_decision_active_keyboard_capture_skips_the_pass() {
    // Capture mode keeps the combos unregistered on purpose (§4.4 v5.5); a
    // focus change must not re-register them while a press is a bind candidate.
    assert!(!should_retry_failed_registrations(
        None,
        Some(true),
        true,
        true
    ));
    assert!(!should_retry_failed_registrations(
        Some(false),
        Some(true),
        true,
        true
    ));
}

#[test]
fn retry_status_update_replaces_the_failed_row_and_keeps_the_rest() {
    let mut status = vec![
        Registration::Failed {
            action: Action::FullScreen,
            combo: "PrintScreen".to_owned(),
            reason: "already registered by another application".to_owned(),
        },
        Registration::Reserved {
            action: Action::Rect,
            combo: "Ctrl+PrintScreen".to_owned(),
        },
    ];

    merge_registration_statuses(
        &mut status,
        vec![Registration::Registered {
            action: Action::FullScreen,
            combo: "PrintScreen".to_owned(),
        }],
    );

    assert_eq!(
        status,
        vec![
            Registration::Registered {
                action: Action::FullScreen,
                combo: "PrintScreen".to_owned(),
            },
            Registration::Reserved {
                action: Action::Rect,
                combo: "Ctrl+PrintScreen".to_owned(),
            },
        ]
    );

    // An action the report did not have yet (a row registered at startup that
    // failed later) is appended, never dropped.
    merge_registration_statuses(
        &mut status,
        vec![Registration::Failed {
            action: Action::Window,
            combo: "Alt+PrintScreen".to_owned(),
            reason: "already registered by another application".to_owned(),
        }],
    );
    assert_eq!(status.len(), 3);
    assert_eq!(status[2].action(), Action::Window);
}

#[test]
fn registration_failure_reason_for_reserved_key_is_unchanged() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The reserved rect row has its own fixed wording (§4.1): a failed combo is
    // never reserved, so the readable-reason change must not touch it. The
    // reserved tooltip renders through catalog `hotkey.reserved_tooltip`.
    let reserved = Registration::Reserved {
        action: Action::Rect,
        combo: "Ctrl+PrintScreen".to_owned(),
    };
    let reserved_icon = icon_for_status(&reserved).expect("info icon");
    assert_eq!(reserved_icon.tooltip, reserved_tooltip());
    assert_en_catalog(
        "hotkey.reserved_tooltip",
        &reserved_icon.tooltip,
        "reserved — out of MVP scope",
    );

    let draft = Settings::default();
    assert_eq!(keyboard_warning(&draft, Action::Rect, None), None);

    // A FAILED row passes its readable reason through verbatim (the draft
    // collision still outranks it — shipped Stage 10 rule). The reason is a
    // passthrough string, not catalog text, so it needs no locale key.
    let readable = "already registered by another application (PrintScreen is held elsewhere) — free that key or pick another";
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, Some(readable)),
        Some(readable.to_owned())
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

/// Stage 3 (spec §2/§7): under the `ru` locale the tab headers render the
/// Russian catalog values. `ru` is restored BEFORE the assertions so a
/// failure never leaves it installed for other tests.
#[test]
fn tab_labels_follow_russian_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let labels = [
        Tab::General.label(),
        Tab::Hotkeys.label(),
        Tab::Overlay.label(),
    ];

    rust_i18n::set_locale("en");

    assert_eq!(
        labels,
        [
            "Общие".to_owned(),
            "Горячие клавиши".to_owned(),
            "Оверлей".to_owned(),
        ],
        "tab headers must render Russian under the ru locale"
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

// ── Auto-update UI decisions (task 2026-11-11_auto-update, Stage 6) ──
// Pure state decisions only: injected values, no network, no self-replace,
// no threads. The wiring in `SettingsApp` (row visibility, dialog buttons,
// discovery toast gate, install-thread result transitions) goes through
// exactly these functions, so each one mirrors one decision in `logic`.

/// A discovery snapshot for the decision tests — every field is a fixed
/// literal, nothing is read from the environment.
fn sample_update_info() -> crate::update::UpdateInfo {
    crate::update::UpdateInfo {
        version: "0.7.0".to_owned(),
        release_date: "2026-11-11T12:00:00.000Z".to_owned(),
        commit_sha: "abc1234".to_owned(),
        archive_url:
            "https://github.com/virshot/virshot/releases/download/v0.7.0/virshot-v0.7.0-win-x64.zip"
                .to_owned(),
        sha512: "AAAA".to_owned(),
        size: 4_200_000,
    }
}

#[test]
fn update_row_visible_with_discovery_shows_row() {
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a held discovery must show the row"
    );
}

#[test]
fn update_row_visible_never_interval_forced_discovery_shows_row() {
    // A discovery can come from the forced «Проверить обновления» button,
    // which works with ANY interval (spec §4.7), so the interval no longer
    // gates the row.
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a discovery from a forced check must show the row with the interval off"
    );
}

#[test]
fn update_row_visible_dev_build_shows_row() {
    // A `-dev` build checks updates on the same terms as a release build
    // (spec §4.7), so its discovery gets the row too.
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a dev build's discovery must show the update row"
    );
}

#[test]
fn update_row_visible_no_info_hides_row() {
    assert!(
        !update_row_visible(None),
        "without a discovery there is nothing to show"
    );
}

#[test]
fn check_now_button_enabled_when_idle() {
    assert!(
        check_now_button_enabled(false),
        "the forced-check button is enabled while no check runs"
    );
}

#[test]
fn check_now_button_enabled_while_in_flight_disabled() {
    assert!(
        !check_now_button_enabled(true),
        "exactly one check may be in flight, so the button is disabled while one runs"
    );
}

#[test]
fn force_check_requested_never_interval_still_requests() {
    // The button ignores the interval setting (spec §4.7): `Never` still runs
    // a forced check.
    assert!(
        force_check_requested(crate::config::UpdateCheck::Never, false),
        "a forced check must run even when scheduled checks are off"
    );
    assert!(
        force_check_requested(crate::config::UpdateCheck::Week, false),
        "a forced check must run on every interval"
    );
}

#[test]
fn force_check_requested_while_in_flight_skips() {
    assert!(
        !force_check_requested(crate::config::UpdateCheck::Never, true),
        "a second check must not be spawned while one is in flight"
    );
}

#[test]
fn install_button_enabled_when_idle() {
    assert!(
        install_button_enabled(&UpdateStatus::Idle),
        "idle is the only state that offers the install action"
    );
}

#[test]
fn install_button_disabled_while_installing() {
    assert!(
        !install_button_enabled(&UpdateStatus::Installing),
        "a running install must not offer a second one"
    );
}

#[test]
fn install_button_disabled_after_failure() {
    let message = "failed to replace running binary: Permission denied (os error 5)";
    let status = UpdateStatus::Failed(message.to_owned());
    assert!(
        !install_button_enabled(&status),
        "a failed install must not re-offer the button in the same state"
    );
    assert_eq!(
        install_failure_hint(&status),
        Some(message),
        "the row must show the verbatim English error text instead of the button"
    );
    assert_eq!(
        install_failure_hint(&UpdateStatus::Idle),
        None,
        "idle has no failure text to show"
    );
    assert_eq!(
        install_failure_hint(&UpdateStatus::Installing),
        None,
        "a running install has no failure text yet"
    );
    assert_eq!(
        install_failure_hint(&UpdateStatus::RestartPending),
        None,
        "a finished install shows no failure text"
    );
}

#[test]
fn dialog_cancel_dismisses_without_install() {
    assert!(
        matches!(
            dialog_outcome(
                UpdateDialogButton::Cancel,
                &UpdateStatus::Idle,
                Some(&sample_update_info())
            ),
            Some(UpdateDialogOutcome::Dismiss)
        ),
        "Cancel closes the dialog and installs nothing"
    );
    assert!(
        matches!(
            dialog_outcome(
                UpdateDialogButton::Cancel,
                &UpdateStatus::Installing,
                Some(&sample_update_info())
            ),
            Some(UpdateDialogOutcome::Dismiss)
        ),
        "Cancel closes regardless of the install state"
    );
}

#[test]
fn dialog_install_request_only_when_idle() {
    // `UpdateInfo` ships `Debug, Clone` with no `PartialEq` (Stage 3), so the
    // transfer is checked by variant plus the short fields the install thread
    // actually uses.
    match dialog_outcome(
        UpdateDialogButton::Install,
        &UpdateStatus::Idle,
        Some(&sample_update_info()),
    ) {
        Some(UpdateDialogOutcome::Install(info)) => {
            assert_eq!(info.version, "0.7.0");
            assert_eq!(info.commit_sha, "abc1234");
            assert_eq!(
                info.archive_url,
                "https://github.com/virshot/virshot/releases/download/v0.7.0/virshot-v0.7.0-win-x64.zip"
            );
        }
        other => {
            panic!("idle + discovery must hand the update info to the install path, got {other:?}")
        }
    }
    assert!(
        dialog_outcome(
            UpdateDialogButton::Install,
            &UpdateStatus::Installing,
            Some(&sample_update_info())
        )
        .is_none(),
        "a second Install press while Installing must request nothing"
    );
    assert!(
        dialog_outcome(UpdateDialogButton::Install, &UpdateStatus::Idle, None).is_none(),
        "Install with no held discovery requests nothing"
    );
    assert!(
        dialog_outcome(
            UpdateDialogButton::Install,
            &UpdateStatus::Failed("boom".to_owned()),
            Some(&sample_update_info())
        )
        .is_none(),
        "Install after a failure requests nothing (the row shows the error instead)"
    );
}

#[test]
fn auto_install_on_discovery_true_only_when_auto_enabled() {
    assert!(
        auto_install_on_discovery(true, true),
        "auto-install ON installs straight away on discovery (spec §4.7)"
    );
    assert!(
        !auto_install_on_discovery(true, false),
        "auto-install OFF keeps the dialog path"
    );
    assert!(
        !auto_install_on_discovery(false, true),
        "nothing is installed without a discovery"
    );
    assert!(
        !auto_install_on_discovery(false, false),
        "nothing is installed without a discovery and without auto-install"
    );
}

#[test]
fn update_toast_enabled_follows_overlay_toggle() {
    assert!(
        update_toast_enabled(Some(&sample_update_info()), true),
        "the discovery chip shows while the overlay is enabled, like the save toast (§4.3)"
    );
    assert!(
        !update_toast_enabled(Some(&sample_update_info()), false),
        "overlay disabled suppresses the success-style discovery chip (§4.3 should_show)"
    );
    assert!(!update_toast_enabled(None, true), "no discovery, no chip");
}

#[test]
fn discovery_clears_after_successful_install() {
    assert_eq!(
        install_finished(&Ok(())),
        UpdateStatus::RestartPending,
        "a successful install ends in the restart-pending state"
    );
    assert_eq!(
        install_finished(&Err(
            "failed to replace running binary: os error 5".to_owned()
        )),
        UpdateStatus::Failed("failed to replace running binary: os error 5".to_owned()),
        "a failed install ends in the Failed state carrying the verbatim message"
    );
    assert!(
        update_info_after_install(Some(sample_update_info()), true).is_none(),
        "success clears the discovery, so the row disappears"
    );
    let kept = update_info_after_install(Some(sample_update_info()), false);
    assert!(
        kept.as_ref().is_some_and(|info| info.version == "0.7.0"
            && info.archive_url
                == "https://github.com/virshot/virshot/releases/download/v0.7.0/virshot-v0.7.0-win-x64.zip"),
        "failure keeps the discovery so the row can show the error"
    );
    assert!(
        update_info_after_install(None, true).is_none(),
        "the auto-install path has no held discovery to clear"
    );
}

#[test]
fn new_discovery_after_failure_restores_install_button() {
    // Stage 7 (spec §4.7): a NEW discovery arriving while a failed install is
    // shown makes the row actionable again — the failure text belongs to the
    // OLD attempt, so the state returns to Idle and Install is offered.
    assert_eq!(
        status_after_new_discovery(UpdateStatus::Failed("download failed".to_owned())),
        UpdateStatus::Idle,
        "a fresh discovery clears a stale install failure"
    );
    assert!(
        install_button_enabled(&status_after_new_discovery(UpdateStatus::Failed(
            "download failed".to_owned()
        ))),
        "the row must offer Install again after a fresh discovery"
    );
    assert_eq!(
        status_after_new_discovery(UpdateStatus::Idle),
        UpdateStatus::Idle,
        "Idle stays Idle"
    );
    assert_eq!(
        status_after_new_discovery(UpdateStatus::Installing),
        UpdateStatus::Installing,
        "a running install must not be interrupted by a new discovery"
    );
    assert_eq!(
        status_after_new_discovery(UpdateStatus::RestartPending),
        UpdateStatus::RestartPending,
        "a finished install stays restart-pending"
    );
}

// ── Background-check slot + schedule decisions (task 2026-11-11_auto-update,
// Stage 9 acceptance fixes) ──
//
// The pass-1 arm/drain race, the forced-press gate and the Apply-time schedule
// are pure data: the helpers take the flags plus the injected clock, so all
// three rules are pinned WITHOUT an eframe context (`App::logic` needs a
// `Context` + `Frame`, which unit tests never build).

#[test]
fn first_pass_outcome_drained_before_arm_leaves_button_enabled() {
    // The startup check spawned by `main.rs` fails fast (HTTP 404) or the
    // window init is slow, so its outcome drains on the FIRST logic pass.
    // Arming the deadline AFTER the drain put `check_in_flight` back ON with
    // nothing running: the slot stayed occupied forever — scheduled checks
    // never fired and «Проверить обновления» stayed disabled all session.
    let startup_deadline =
        update::next_check_deadline(Instant::now(), crate::config::UpdateCheck::Day.interval());
    assert!(
        startup_deadline.is_some(),
        "a scheduled interval arms a deadline on the first pass"
    );
    let after_drain = check_in_flight_after(false, true, startup_deadline);
    assert!(
        !after_drain,
        "a drained startup outcome frees the slot even though this pass armed a deadline"
    );
    assert!(
        check_now_button_enabled(after_drain),
        "«Проверить обновления» stays enabled after the pass-1 drain"
    );
    assert!(
        check_in_flight_after(false, false, startup_deadline),
        "with no outcome drained, the first-pass deadline IS the startup check (§4.7)"
    );
    assert!(
        !check_in_flight_after(false, false, None),
        "a Never interval arms nothing and spawns nothing, so the slot stays free"
    );
}

#[test]
fn scheduled_spawn_keeps_one_check_in_flight() {
    // A scheduled spawn consumes its deadline (`next_check = None`) and holds
    // the single slot until the outcome drains: no second spawn and no forced
    // check can start meanwhile (§4.7 — exactly one check in flight).
    let running = check_in_flight_after(true, false, None);
    assert!(
        running,
        "a check spawned by an earlier pass stays in flight on a pass with no outcome"
    );
    assert!(
        !check_now_button_enabled(running),
        "the forced-check button is disabled while a scheduled check runs"
    );
    assert!(
        !check_in_flight_after(false, false, None),
        "a deadline armed on a LATER pass is a future check, not a running one"
    );
    let freed = check_in_flight_after(true, true, None);
    assert!(
        !freed,
        "the drained outcome ends the running check, so the slot frees"
    );
    assert!(
        check_now_button_enabled(freed),
        "the button re-enables as soon as the outcome drains"
    );
}

#[test]
fn forced_outcome_does_not_rearm_deadline() {
    // §4.7: a FORCED outcome never moves or re-arms the interval deadline;
    // only a SCHEDULED outcome re-arms it to the applied interval.
    let now = Instant::now();
    let armed = update::next_check_deadline(now, crate::config::UpdateCheck::Hour.interval());
    let deadline_after_forced: Option<Instant> = if deadline_rearmed_by_outcome(true) {
        update::next_check_deadline(now, crate::config::UpdateCheck::Week.interval())
    } else {
        armed
    };
    assert_eq!(
        deadline_after_forced, armed,
        "a forced outcome leaves the armed 1h deadline exactly where it was"
    );
    assert!(
        deadline_rearmed_by_outcome(false),
        "a scheduled outcome re-arms the deadline to the applied interval"
    );
    assert!(
        update::next_check_deadline(now, crate::config::UpdateCheck::Never.interval()).is_none(),
        "a Never interval re-arms nothing"
    );
}

#[test]
fn forced_press_while_slot_busy_stays_pending() {
    // A press the paint recorded while a scheduled spawn took the slot on the
    // SAME logic pass must not be discarded: it stays pending and runs as soon
    // as the slot frees (the old code cleared the request before the gate).
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Day, true, true),
        ForcedCheckDecision {
            spawn: false,
            pending: true
        },
        "a busy slot defers the press instead of swallowing it"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Day, true, false),
        ForcedCheckDecision {
            spawn: true,
            pending: false
        },
        "a free slot honors the press on this pass"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Never, true, false),
        ForcedCheckDecision {
            spawn: true,
            pending: false
        },
        "«никогда» still runs a forced check — the interval is ignored (§4.7)"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Never, true, true),
        ForcedCheckDecision {
            spawn: false,
            pending: true
        },
        "a busy slot defers a Never-interval press too"
    );
    assert_eq!(
        forced_check_decision(crate::config::UpdateCheck::Day, false, false),
        ForcedCheckDecision {
            spawn: false,
            pending: false
        },
        "no press, no effect"
    );
}

#[test]
fn apply_never_interval_clears_deadline() {
    let now = Instant::now();
    assert!(
        schedule_after_apply(now, crate::config::UpdateCheck::Day).is_some(),
        "a scheduled interval keeps the deadline armed through Apply"
    );
    assert!(
        schedule_after_apply(now, crate::config::UpdateCheck::Never).is_none(),
        "switching to «никогда» clears an armed deadline, so no extra check fires (§4.7)"
    );
}

#[test]
fn apply_scheduled_interval_arms_deadline() {
    let now = Instant::now();
    assert_eq!(
        schedule_after_apply(now, crate::config::UpdateCheck::Day),
        Some(now + Duration::from_secs(86_400)),
        "switching from «никогда» to a scheduled interval arms the background schedule at Apply — no restart needed (§4.7)"
    );
    assert_eq!(
        schedule_after_apply(now, crate::config::UpdateCheck::Hour),
        Some(now + Duration::from_secs(3600)),
        "the armed deadline follows the APPLIED interval, not the draft"
    );
}

#[test]
fn apply_interval_change_rearms_deadline_to_new_interval() {
    let now = Instant::now();
    let old_deadline = schedule_after_apply(now, crate::config::UpdateCheck::Hour);
    let new_deadline = schedule_after_apply(now, crate::config::UpdateCheck::Week);
    assert_eq!(
        new_deadline,
        Some(now + Duration::from_secs(604_800)),
        "a 1h→1w switch re-arms the deadline to the NEW interval"
    );
    assert!(
        new_deadline > old_deadline,
        "the switch must push the deadline out, not keep the stale 1h one"
    );
}

// ── Forced-check result line + stale-discovery reset (task
// 2026-11-11_auto-update, Stage 11 sub-items 2+4) ──
//
// Pure state decisions again — injected outcomes, flags, and install status,
// no eframe context, no threads, no network. `SettingsApp::logic` drains
// through exactly these helpers (mirroring how the Stage 9 slot tests pin the
// drain rules without ever building a `Context`).

/// The verbatim `UpdateError` Display of a stable-channel 404 — the exact
/// string the failed-forced-check line must show (never localized, the
/// `capture::error_display` precedent).
fn stable_404_error() -> crate::update::UpdateError {
    crate::update::UpdateError::HttpStatus(404)
}

const STABLE_404_TEXT: &str = "release metadata fetch returned HTTP status 404";

#[test]
fn forced_check_status_after_forced_up_to_date_shows_no_updates_line() {
    assert_eq!(
        forced_check_status_after(None, &crate::update::CheckOutcome::UpToDate, true),
        Some(ForcedCheckStatus::NoUpdates),
        "a forced up-to-date outcome sets the «Обновлений нет» line"
    );
    // Before ANY forced check the slot stays empty — no status text ever.
    assert_eq!(
        forced_check_status_after(None, &crate::update::CheckOutcome::UpToDate, false),
        None,
        "a scheduled outcome never fills the slot before the first forced check"
    );
}

#[test]
fn forced_check_status_after_forced_failure_shows_verbatim_error_text() {
    assert_eq!(
        forced_check_status_after(
            None,
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true
        ),
        Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
        "the line must carry the exact `UpdateError` Display, never localized"
    );
}

#[test]
fn forced_check_status_after_forced_discovery_clears_slot() {
    // The discovery itself replaces the result line: a stale «Обновлений нет»
    // or error text must never sit next to a fresh «Доступно обновление».
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::NoUpdates),
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            true
        ),
        None,
        "a forced discovery clears the result line"
    );
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            true
        ),
        None,
        "a forced discovery clears the previous error text too"
    );
}

#[test]
fn forced_check_status_after_scheduled_outcome_keeps_slot() {
    // Background/scheduled results keep using the toast + General row (§4.7);
    // they never touch the beside-button slot in either direction.
    let held = Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned()));
    assert_eq!(
        forced_check_status_after(held.clone(), &crate::update::CheckOutcome::UpToDate, false),
        held,
        "a scheduled UpToDate leaves the held forced line untouched"
    );
    assert_eq!(
        forced_check_status_after(
            held.clone(),
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            false
        ),
        held,
        "a scheduled failure leaves the held forced line untouched"
    );
    assert_eq!(
        forced_check_status_after(
            held.clone(),
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            false
        ),
        held,
        "a scheduled discovery leaves the held forced line untouched"
    );
}

#[test]
fn forced_failure_replaces_earlier_no_updates_line() {
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::NoUpdates),
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true
        ),
        Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
        "a later forced failure replaces the earlier «Обновлений нет»"
    );
}

#[test]
fn forced_up_to_date_replaces_earlier_failure_text() {
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
            &crate::update::CheckOutcome::UpToDate,
            true
        ),
        Some(ForcedCheckStatus::NoUpdates),
        "a later forced success replaces the earlier error text"
    );
}

#[test]
fn forced_check_in_flight_shows_no_status_text() {
    let slot = Some(ForcedCheckStatus::NoUpdates);
    assert_eq!(
        forced_check_status_shown(slot.as_ref(), true),
        None,
        "while a FORCED check runs, the row shows no status text (the slot stays untouched until its outcome drains)"
    );
    assert_eq!(
        forced_check_status_shown(slot.as_ref(), false),
        Some(&ForcedCheckStatus::NoUpdates),
        "once the slot is free, the held line renders"
    );
    assert_eq!(
        forced_check_status_shown(None, false),
        None,
        "before the first forced check there is nothing to render"
    );
}

#[test]
fn discovery_cleared_by_forced_up_to_date_outcome() {
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            true,
            &UpdateStatus::Idle
        ),
        "an up-to-date forced outcome means nothing newer exists — the held discovery must go"
    );
}

#[test]
fn discovery_cleared_by_forced_failure_after_held_nightly_discovery() {
    // The user's bug: nightly discovery held, channel switched to `stable`,
    // the forced check answers 404 — «Доступно обновление» must disappear.
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true,
            &UpdateStatus::Idle
        ),
        "a failed FORCED check replaces the previous result and clears the stale discovery"
    );
}

#[test]
fn discovery_kept_by_scheduled_failure_outcome() {
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            false,
            &UpdateStatus::Idle
        ),
        "a transient scheduled blip (network/rate-limit) must not erase an actionable update"
    );
}

#[test]
fn discovery_cleared_by_scheduled_up_to_date_outcome() {
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            false,
            &UpdateStatus::Idle
        ),
        "a scheduled up-to-date outcome clears the held discovery too (§4.7)"
    );
}

#[test]
fn discovery_never_cleared_while_install_in_progress() {
    // A running install owns its discovery: no check outcome — forced,
    // scheduled, success or failure — may drop it mid-install.
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            true,
            &UpdateStatus::Installing
        ),
        "no outcome clears while an install runs"
    );
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::UpToDate,
            false,
            &UpdateStatus::Installing
        ),
        "not even a scheduled up-to-date outcome"
    );
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true,
            &UpdateStatus::Installing
        ),
        "a forced failure must not drop the discovery a live install needs"
    );
    assert!(
        !discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Updated(sample_update_info()),
            true,
            &UpdateStatus::Idle
        ),
        "`Updated` never clears here — the drain REPLACES the discovery (install failure keeps / install success clear stay in `update_info_after_install`, pinned by `discovery_clears_after_successful_install`)"
    );
}

#[test]
fn forced_failure_resets_stale_discovery_and_hides_update_row() {
    // End-to-end decision chain the drain applies: held nightly discovery +
    // forced 404 on `stable` → clear → «Доступно обновление» hidden.
    let held = Some(sample_update_info());
    assert!(
        discovery_cleared_by_check_outcome(
            &crate::update::CheckOutcome::Failed(stable_404_error()),
            true,
            &UpdateStatus::Idle
        ),
        "fixture: the forced failure resets the held discovery"
    );
    let cleared = if discovery_cleared_by_check_outcome(
        &crate::update::CheckOutcome::Failed(stable_404_error()),
        true,
        &UpdateStatus::Idle,
    ) {
        None
    } else {
        held
    };
    assert!(
        !update_row_visible(cleared.as_ref()),
        "the reset slot must hide the «Доступно обновление» row"
    );
}

#[test]
fn forced_discovery_keeps_row_visible_and_clears_status_line() {
    // The preserved behavior: a forced success WITH an update shows the row;
    // the beside-button line steps aside for it (sub-item 2 slot rule).
    let outcome = crate::update::CheckOutcome::Updated(sample_update_info());
    assert!(
        !discovery_cleared_by_check_outcome(&outcome, true, &UpdateStatus::Idle),
        "a forced discovery must not clear — it replaces the held discovery"
    );
    assert!(
        update_row_visible(Some(&sample_update_info())),
        "a forced success-with-update keeps the row visible"
    );
    assert_eq!(
        forced_check_status_after(
            Some(ForcedCheckStatus::Failed(STABLE_404_TEXT.to_owned())),
            &outcome,
            true
        ),
        None,
        "the stale error line must go as soon as the discovery row appears"
    );
}
