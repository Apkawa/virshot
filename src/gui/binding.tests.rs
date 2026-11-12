use super::*;

use crate::config::Controls;
use global_hotkey::hotkey::Code;
use global_hotkey::hotkey::Modifiers;
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

/// Assert a rendered UI string goes through the catalog `key` AND the
/// catalog's `en` value equals the shipped English contract byte-for-byte.
/// Callers must hold `locale_guard()` and pin `"en"` first — this helper
/// calls `t!` itself. Tests that never leave `en` need no further restore:
/// `crate::LOCALE_LOCK` serializes locale mutations process-wide, so no other
/// test changes the locale while this test holds it.
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

fn default_controls() -> Controls {
    Settings::default().controls
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
