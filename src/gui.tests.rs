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
        assert!(icon_glyph(kind).chars().all(|c| c.is_ascii()));
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

fn monitor(x: f32, y: f32) -> Option<egui::Vec2> {
    Some(egui::Vec2::new(x, y))
}

fn toast_size() -> egui::Vec2 {
    egui::Vec2::new(200.0, 36.0)
}

#[test]
fn toast_position_bottom_right_offsets_30_60_clear_taskbar() {
    // Empirical tk-reference offsets (spec §4.3 v6.2): 30 px from the
    // vertical border, 60 px from the horizontal border — the bottom
    // corners sit above the Windows taskbar (~48 px) without OS APIs.
    let position = toast_position(
        monitor(1920.0, 1080.0),
        OverlayPosition::BottomRight,
        toast_size(),
    )
    .expect("position");
    assert_eq!(
        position,
        egui::Pos2::new(
            1920.0 - 200.0 - TOAST_OFFSET_X,
            1080.0 - 36.0 - TOAST_OFFSET_Y
        )
    );
}

#[test]
fn toast_position_top_left_offsets_30_x_60_y() {
    let position = toast_position(
        monitor(1920.0, 1080.0),
        OverlayPosition::TopLeft,
        toast_size(),
    )
    .expect("position");
    assert_eq!(position, egui::Pos2::new(TOAST_OFFSET_X, TOAST_OFFSET_Y));
}

#[test]
fn toast_position_top_right_and_bottom_left_anchor_the_other_corners() {
    let top_right = toast_position(
        monitor(1920.0, 1080.0),
        OverlayPosition::TopRight,
        toast_size(),
    )
    .expect("position");
    let bottom_left = toast_position(
        monitor(1920.0, 1080.0),
        OverlayPosition::BottomLeft,
        toast_size(),
    )
    .expect("position");
    assert_eq!(
        top_right,
        egui::Pos2::new(1920.0 - 200.0 - TOAST_OFFSET_X, TOAST_OFFSET_Y)
    );
    assert_eq!(
        bottom_left,
        egui::Pos2::new(TOAST_OFFSET_X, 1080.0 - 36.0 - TOAST_OFFSET_Y)
    );
}

#[test]
fn toast_position_is_none_without_monitor_size() {
    // Wayland reports no monitor size — placement stays with the compositor.
    assert_eq!(
        toast_position(None, OverlayPosition::BottomRight, toast_size()),
        None
    );
}

#[test]
fn toast_position_clamps_to_the_screen_on_a_tiny_monitor() {
    // A monitor smaller than the toast keeps the toast at the origin
    // rather than pushing it off-screen with a negative position.
    let position = toast_position(
        monitor(100.0, 40.0),
        OverlayPosition::BottomRight,
        toast_size(),
    )
    .expect("position");
    assert_eq!(position, egui::Pos2::new(0.0, 0.0));
}

fn monitor_rect(x: f32, y: f32, width: f32, height: f32, primary: bool) -> MonitorRect {
    MonitorRect {
        x,
        y,
        width,
        height,
        primary,
    }
}

#[test]
fn toast_positions_bottomright_per_monitor() {
    // `on_all_monitors` (spec §4.3 v5.5): one placement per monitor, same
    // corner, each anchored to THAT monitor's own corner with the
    // empirical 30/60 offsets.
    let monitors = [
        monitor_rect(0.0, 0.0, 1920.0, 1080.0, true),
        monitor_rect(1920.0, 0.0, 2560.0, 1080.0, false),
    ];
    assert_eq!(
        toast_positions(
            &monitors,
            OverlayPosition::BottomRight,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true,
        ),
        vec![
            egui::Pos2::new(
                1920.0 - 200.0 - TOAST_OFFSET_X,
                1080.0 - 36.0 - TOAST_OFFSET_Y
            ),
            egui::Pos2::new(
                1920.0 + 2560.0 - 200.0 - TOAST_OFFSET_X,
                1080.0 - 36.0 - TOAST_OFFSET_Y
            ),
        ]
    );
}

#[test]
fn toast_positions_topleft_no_negative_offset() {
    // A monitor LEFT of the primary has negative desktop coordinates: its
    // TopLeft toast stays inside that monitor, it is not clamped to 0.
    let left_monitor = [monitor_rect(-1920.0, 0.0, 1920.0, 1080.0, false)];
    assert_eq!(
        toast_positions(
            &left_monitor,
            OverlayPosition::TopLeft,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true,
        ),
        vec![egui::Pos2::new(-1920.0 + TOAST_OFFSET_X, TOAST_OFFSET_Y)]
    );

    // A monitor smaller than the toast keeps the toast at the monitor
    // origin rather than pushing it off-monitor with a negative offset.
    let tiny = [monitor_rect(0.0, 0.0, 100.0, 40.0, true)];
    assert_eq!(
        toast_positions(
            &tiny,
            OverlayPosition::BottomRight,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true
        ),
        vec![egui::Pos2::new(0.0, 0.0)]
    );
}

#[test]
fn toast_positions_single_primary_when_disabled() {
    // `on_all_monitors = false`: exactly one toast, on the PRIMARY monitor
    // (spec §4.3), even when it is not the first in the enumeration.
    let monitors = [
        monitor_rect(0.0, 0.0, 1920.0, 1080.0, false),
        monitor_rect(1920.0, 0.0, 1024.0, 768.0, true),
    ];
    assert_eq!(
        toast_positions(
            &monitors,
            OverlayPosition::TopRight,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            false,
        ),
        vec![egui::Pos2::new(
            1920.0 + 1024.0 - 200.0 - TOAST_OFFSET_X,
            TOAST_OFFSET_Y
        )]
    );

    // No monitor marked primary → the first one; no monitors → no placement.
    assert_eq!(
        toast_positions(
            &[monitor_rect(0.0, 0.0, 800.0, 600.0, false)],
            OverlayPosition::TopLeft,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            false,
        ),
        vec![egui::Pos2::new(TOAST_OFFSET_X, TOAST_OFFSET_Y)]
    );
    assert_eq!(
        toast_positions(
            &[],
            OverlayPosition::BottomRight,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true
        ),
        Vec::new()
    );
}

#[test]
fn toast_positions_all_corners_offsets_30_x_60_y() {
    // Empirical per-axis offsets (spec §4.3 v6.2): 30 px from the vertical
    // border, 60 px from the horizontal border, on all four corners — the
    // offsets are parameters of the helper, not baked into the math.
    let monitors = [monitor_rect(0.0, 0.0, 1920.0, 1080.0, true)];
    let right = 1920.0 - 200.0 - TOAST_OFFSET_X;
    let bottom = 1080.0 - 36.0 - TOAST_OFFSET_Y;

    assert_eq!(
        toast_positions(
            &monitors,
            OverlayPosition::TopLeft,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true
        ),
        vec![egui::Pos2::new(TOAST_OFFSET_X, TOAST_OFFSET_Y)]
    );
    assert_eq!(
        toast_positions(
            &monitors,
            OverlayPosition::TopRight,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true
        ),
        vec![egui::Pos2::new(right, TOAST_OFFSET_Y)]
    );
    assert_eq!(
        toast_positions(
            &monitors,
            OverlayPosition::BottomLeft,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true
        ),
        vec![egui::Pos2::new(TOAST_OFFSET_X, bottom)]
    );
    assert_eq!(
        toast_positions(
            &monitors,
            OverlayPosition::BottomRight,
            toast_size(),
            TOAST_OFFSET_X,
            TOAST_OFFSET_Y,
            true
        ),
        vec![egui::Pos2::new(right, bottom)]
    );
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
fn toast_width_grows_with_text_and_stays_clamped() {
    assert_eq!(toast_width("Saved: a.png"), TOAST_MIN_WIDTH);
    assert!(toast_width("Saved: very-long-file-name.png") > TOAST_MIN_WIDTH);
    assert_eq!(toast_width(&"x".repeat(500)), TOAST_MAX_WIDTH);
}

#[test]
fn viewport_has_window_without_input_entry_is_false() {
    // The spawn pass: the toast viewport is asked for but the backend has
    // no window for it yet, so its repaint request must not be spent here.
    let raw = egui::RawInput::default();
    assert!(!viewport_has_window(&raw, toast_viewport_id(1, 0)));
}

#[test]
fn viewport_has_window_with_input_entry_is_true() {
    let mut raw = egui::RawInput::default();
    raw.viewports
        .insert(toast_viewport_id(2, 1), egui::ViewportInfo::default());
    assert!(viewport_has_window(&raw, toast_viewport_id(2, 1)));
    assert!(!viewport_has_window(&raw, toast_viewport_id(2, 0)));
}

/// Placements of a spawn: `count` of them, same corner.
fn placements(count: usize) -> Vec<Option<egui::Pos2>> {
    vec![Some(egui::Pos2::new(10.0, 20.0)); count]
}

/// Live windows of one generation, `count` of them.
fn live_windows(generation: u64, count: usize) -> Vec<ToastViewport> {
    toast_windows(generation, &placements(count))
}

#[test]
fn toast_window_step_replacement_defers_spawn_until_old_windows_close() {
    // The replacement pass: the OLD window is closed and the fresh one is
    // queued, NOT shown. eframe creates new windows before destroying the
    // closed ones (`handle_viewport_output`), so a same-pass spawn would
    // put the old (unpainted, white) window and the new one on screen
    // together — the "toast drawn from two windows" the user saw.
    let live = live_windows(1, 1);
    let step = toast_window_step(&live, None, Some(placements(1)), true, 2);
    assert_eq!(step.close, vec![live[0].id]);
    assert!(step.show.is_empty(), "no spawn in the close pass");
    assert!(!step.spawned);
    assert_eq!(step.pending, Some(placements(1)));
}

#[test]
fn toast_window_step_after_close_spawns_queued_window_once() {
    // The pass after the close: the queue drains into exactly ONE window
    // with a fresh id, and nothing stays queued.
    let step = toast_window_step(&[], Some(placements(1)), None, true, 2);
    assert!(step.close.is_empty());
    assert_eq!(step.show.len(), 1, "exactly one toast window");
    assert_eq!(step.show[0].id, toast_viewport_id(2, 0));
    assert_ne!(step.show[0].id, toast_viewport_id(1, 0), "ids never reused");
    assert!(step.spawned);
    assert_eq!(step.pending, None);
}

#[test]
fn toast_window_step_spawn_without_live_windows_is_immediate() {
    // A first toast (nothing live) spawns in the very same pass.
    let step = toast_window_step(&[], None, Some(placements(1)), true, 3);
    assert!(step.close.is_empty());
    assert_eq!(step.show.len(), 1);
    assert!(step.spawned);
    assert_eq!(step.pending, None);
}

#[test]
fn toast_window_step_multi_monitor_spawns_one_window_per_placement() {
    // `overlay.on_all_monitors`: still ONE window per monitor, no duplicates.
    let step = toast_window_step(&[], Some(placements(2)), None, true, 4);
    assert_eq!(step.show.len(), 2);
    assert_eq!(step.show[0].id, toast_viewport_id(4, 0));
    assert_eq!(step.show[1].id, toast_viewport_id(4, 1));
    assert_ne!(step.show[0].id, step.show[1].id);
}

#[test]
fn toast_window_step_newer_notification_replaces_the_queued_placement() {
    // Newest wins (spec §4.3): a second notification while the spawn is
    // still queued re-queues its placements instead of adding a window.
    let live = live_windows(8, 1);
    let step = toast_window_step(&live, Some(placements(2)), Some(placements(1)), true, 9);
    assert_eq!(step.close, vec![live[0].id]);
    assert!(step.show.is_empty());
    assert_eq!(step.pending, Some(placements(1)));
}

#[test]
fn toast_window_step_unchanged_pass_keeps_the_live_windows() {
    let live = live_windows(7, 1);
    let step = toast_window_step(&live, None, None, true, 7);
    assert!(step.close.is_empty());
    assert_eq!(step.show.len(), 1);
    assert_eq!(step.show[0].id, live[0].id);
    assert!(!step.spawned);
    assert_eq!(step.pending, None);
}

#[test]
fn toast_window_step_finished_toast_closes_windows_and_drops_pending_spawn() {
    // A toast that expired before its spawn pass must never appear.
    let live = live_windows(5, 1);
    let step = toast_window_step(&live, Some(placements(1)), None, false, 6);
    assert_eq!(step.close, vec![live[0].id]);
    assert!(step.show.is_empty());
    assert_eq!(step.pending, None, "a finished toast never spawns");
}

#[test]
fn toast_show_commands_before_paint_is_paint_while_hidden_only() {
    // An unpainted window stays HIDDEN: asking to paint it while hidden is
    // the only command, so the white unpainted glass is never shown.
    assert_eq!(
        toast_show_commands(false),
        vec![egui::ViewportCommand::RequestPaintWhileHidden]
    );
}

#[test]
fn toast_show_commands_after_paint_shows_the_window() {
    // Once the ui callback ran, the window has pixels: show it, still
    // holding paint-alive for the following frames (the fade-out).
    assert_eq!(
        toast_show_commands(true),
        vec![
            egui::ViewportCommand::RequestPaintWhileHidden,
            egui::ViewportCommand::Visible(true)
        ]
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
