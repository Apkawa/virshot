use super::*;

use crate::config::Controls;
use global_hotkey::hotkey::Code;
use global_hotkey::hotkey::Modifiers;

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
    assert_eq!(
        bind_button_label(true, "PrintScreen", &[]),
        CAPTURE_BUTTON_LABEL
    );
}

#[test]
fn bind_button_label_of_a_cleared_binding_shows_the_click_prompt() {
    let mut binding = Binding::default();
    clear_key_binding(&mut binding);

    assert_eq!(
        bind_button_label(false, &binding.key, &binding.mods),
        KEY_UNBOUND_LABEL,
        "an unbound row must say it needs a press, not paint an empty button"
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
    let reserved = Registration::Reserved {
        action: Action::Rect,
        combo: "ctrl+PrintScreen".to_owned(),
    };

    let icon = icon_for_status(&reserved).expect("info icon");
    assert_eq!(icon.kind, IconKind::Info);
    assert!(icon.tooltip.contains("reserved"));
    assert!(icon.tooltip.contains("out of MVP scope"));
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
    let mut draft = Settings::default();
    // Window loses its Alt modifier: both rows are bare `PrintScreen`.
    draft.controls.window = binding("PrintScreen", &[], None);
    let duplicates = duplicate_bindings(&draft);
    assert_eq!(duplicates.len(), 2, "one collision reports both rows");
    assert_eq!(duplicates[0].action, Action::FullScreen);
    assert_eq!(duplicates[0].other, Action::Window);
    assert_eq!(duplicates[0].kind, DuplicateKind::Keyboard);
    assert!(duplicates[0].description.contains("Window"));
    assert!(duplicates[0].description.contains("PrintScreen"));
    assert!(
        duplicates[0].description.is_ascii(),
        "tooltip text must be English"
    );
    assert_eq!(duplicates[1].action, Action::Window);
    assert_eq!(duplicates[1].other, Action::FullScreen);
    assert!(duplicates[1].description.contains("Full screen"));
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
    let mut draft = Settings::default();
    // Keyboard stays unique (§7 defaults); only the pad strings collide.
    draft.controls.full_screen.pad = Some("Guide".to_owned());
    draft.controls.window.pad = Some("guide".to_owned());
    let duplicates = duplicate_bindings(&draft);
    assert_eq!(duplicates.len(), 2, "one pad collision reports both rows");
    assert!(duplicates.iter().all(|d| d.kind == DuplicateKind::Pad));
    assert!(duplicates[0].description.contains("Window pad"));
    assert!(duplicates[1].description.contains("guide"));
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
    let mut draft = Settings::default();
    // Both active rows are bare `PrintScreen` now — the keyboard collision.
    draft.controls.window = binding("PrintScreen", &[], None);
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, None),
        Some("Duplicates Window: PrintScreen".to_owned()),
        "the tooltip must name the other row and this row's combo"
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
    let mut draft = Settings::default();
    draft.controls.window = binding("PrintScreen", &[], None);
    // A collision blocks Apply — that is the actionable state, so it wins over
    // the stale registration status of the last applied settings.
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, Some("already registered")),
        Some("Duplicates Window: PrintScreen".to_owned()),
        "a draft collision outranks a registration failure"
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
    let mut draft = Settings::default();
    // Keyboard stays unique (§7 defaults); only the pad notation collides.
    draft.controls.full_screen.pad = Some("XboxX".to_owned());
    draft.controls.window.pad = Some("xboxx".to_owned());
    assert_eq!(
        pad_warning(&draft, Action::FullScreen),
        Some("Duplicates Window pad: XboxX".to_owned()),
        "the tooltip must name the other row and this row's pad"
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
    // Disabled Apply/OK must tell the user what to do (UI text is English,
    // spec §2). Exact wording, like the other label tests.
    assert_eq!(
        DUPLICATE_SAVE_TOOLTIP,
        "Duplicate binding: one combo or pad button must trigger only one action — re-bind one of the rows"
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
    assert_eq!(Tab::General.label(), "General");
    assert_eq!(Tab::Hotkeys.label(), "Hotkeys");
    assert_eq!(Tab::Overlay.label(), "Overlay");
}

#[test]
fn row_labels_are_english_per_spec() {
    assert_eq!(row_label(Action::FullScreen), "Full screen");
    assert_eq!(row_label(Action::Window), "Window");
    assert_eq!(row_label(Action::Rect), "Rect area");
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
    // One English line at the start of the bind form, painted on every pass
    // (§4.4 v8: it is not gated by capture state); exact wording, and English
    // per spec §2 (ASCII pins it without Cyrillic creeping in).
    assert_eq!(BIND_HINT, "Esc to cancel, Backspace to clear");
    assert!(
        BIND_HINT.is_ascii(),
        "UI text is English (spec §2 i18n note)"
    );
}

#[test]
fn unbound_save_tooltip_explains_why_saving_is_blocked() {
    // Disabled Apply/OK must tell the user what to do (UI text is English,
    // spec §2). Exact wording, like the other label tests.
    assert_eq!(
        UNBOUND_SAVE_TOOLTIP,
        "Bind a key on every row before saving — Backspace clears a binding"
    );
    assert!(
        UNBOUND_SAVE_TOOLTIP.contains("Backspace"),
        "the tooltip must explain how the row got unbound"
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
    assert_eq!(
        pad_field_label(false, false, Some("South")),
        GAMEPAD_PLACEHOLDER_LABEL
    );
    assert_eq!(
        pad_field_label(false, true, Some("South")),
        GAMEPAD_PLACEHOLDER_LABEL
    );
}

#[test]
fn pad_field_label_capturing_shows_press_prompt() {
    assert_eq!(
        pad_field_label(true, true, Some("South")),
        PAD_CAPTURE_BUTTON_LABEL
    );
}

#[test]
fn pad_field_label_shows_notation_or_click_to_bind_prompt() {
    assert_eq!(pad_field_label(true, false, Some("unknown:3")), "unknown:3");
    assert_eq!(pad_field_label(true, false, Some("Guide")), "Guide");
    assert_eq!(pad_field_label(true, false, None), PAD_UNBOUND_LABEL);
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
    // The pad field must state the same affordances as the shared hint line
    // (§4.4, v2). Exact English wording, like the other label tests.
    assert_eq!(
        PAD_CAPTURE_TOOLTIP,
        "Press a gamepad button to bind it — Esc to cancel, Backspace to clear; click again to cancel."
    );
    assert!(
        PAD_CAPTURE_TOOLTIP.contains("Esc to cancel")
            && PAD_CAPTURE_TOOLTIP.contains("Backspace to clear"),
        "pad capture tooltip must name Esc and Backspace: {PAD_CAPTURE_TOOLTIP}"
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
    // The rows ARE `template::TOKENS` — one per entry, token spelled with the
    // `%`, description copied verbatim. A UI-side copy would drift.
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
    assert_eq!(
        overlay_test_message(OVERLAY_TEST_FILENAME),
        "Saved: test.png"
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
/// names the checkbox the user must tick before the test chip is possible.
#[test]
fn overlay_test_disabled_hint_names_the_overlay_checkbox() {
    assert_eq!(
        OVERLAY_TEST_DISABLED_HINT,
        "Enable the overlay toast to test it"
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
    // The reserved rect row has its own fixed wording (§4.1): a failed combo is
    // never reserved, so the readable-reason change must not touch it.
    let reserved = Registration::Reserved {
        action: Action::Rect,
        combo: "Ctrl+PrintScreen".to_owned(),
    };
    assert_eq!(
        icon_for_status(&reserved).expect("info icon").tooltip,
        RESERVED_TOOLTIP
    );

    let draft = Settings::default();
    assert_eq!(keyboard_warning(&draft, Action::Rect, None), None);

    // A FAILED row passes its readable reason through verbatim (the draft
    // collision still outranks it — shipped Stage 10 rule).
    let readable = "already registered by another application (PrintScreen is held elsewhere) — free that key or pick another";
    assert_eq!(
        keyboard_warning(&draft, Action::FullScreen, Some(readable)),
        Some(readable.to_owned())
    );
}
