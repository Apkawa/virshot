use super::*;

fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
    let i = (y * 32 + x) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn luminance(pixel: [u8; 4]) -> u32 {
    (pixel[0] as u32) + (pixel[1] as u32) + (pixel[2] as u32)
}

#[test]
fn camera_glyph_is_32x32_with_nonempty_pixels() {
    let rgba = camera_glyph_rgba();
    assert_eq!(rgba.len(), 32 * 32 * 4);
    assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
}

#[test]
fn camera_glyph_has_lens_darker_than_body() {
    let rgba = camera_glyph_rgba();
    let lens = pixel(&rgba, 16, 16);
    let body = pixel(&rgba, 25, 20);
    assert_eq!(lens[3], 255, "lens core must be opaque");
    assert_eq!(body[3], 255, "body must be opaque");
    assert!(luminance(lens) < luminance(body));
}

#[test]
fn camera_glyph_background_is_transparent() {
    let rgba = camera_glyph_rgba();
    assert_eq!(pixel(&rgba, 0, 0), [0, 0, 0, 0]);
    assert_eq!(pixel(&rgba, 31, 31), [0, 0, 0, 0]);
}

#[test]
fn tray_action_mapping_left_click_up_shows_settings() {
    assert_eq!(
        action_for_click(MouseButton::Left, MouseButtonState::Up),
        Some(TrayAction::ShowSettings)
    );
}

#[test]
fn tray_action_mapping_press_and_secondary_buttons_are_ignored() {
    // Windows sends Down before Up per click; only Up is an action.
    assert_eq!(
        action_for_click(MouseButton::Left, MouseButtonState::Down),
        None
    );
    // Right/middle clicks belong to the menu path, not the window reopen.
    assert_eq!(
        action_for_click(MouseButton::Right, MouseButtonState::Up),
        None
    );
    assert_eq!(
        action_for_click(MouseButton::Middle, MouseButtonState::Up),
        None
    );
}

#[test]
fn tray_action_mapping_menu_ids_map_to_actions() {
    assert_eq!(
        action_for_menu_id(&MenuId(MENU_ID_TAKE_SCREENSHOT.to_owned())),
        Some(TrayAction::TakeScreenshot)
    );
    assert_eq!(
        action_for_menu_id(&MenuId(MENU_ID_SETTINGS.to_owned())),
        Some(TrayAction::ShowSettings)
    );
    assert_eq!(
        action_for_menu_id(&MenuId(MENU_ID_EXIT.to_owned())),
        Some(TrayAction::Exit)
    );
    assert_eq!(action_for_menu_id(&MenuId("other".to_owned())), None);
}

#[test]
fn menu_labels_match_spec_section_4_5() {
    assert_eq!(MENU_LABEL_TAKE_SCREENSHOT, "Take screenshot");
    assert_eq!(MENU_LABEL_SETTINGS, "Settings");
    assert_eq!(MENU_LABEL_EXIT, "Exit");
}
