use super::*;

use crate::config::Controls;

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
}

#[test]
fn supported_key_name_rejects_unsupported_keys() {
    assert_eq!(supported_key_name(egui::Key::Escape), None);
    assert_eq!(supported_key_name(egui::Key::Space), None);
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
