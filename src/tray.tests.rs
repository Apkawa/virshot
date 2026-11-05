use super::*;

/// Read one RGBA pixel from a row-major buffer of the given width.
fn pixel(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    let i = (y * width + x) * 4;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

#[test]
fn app_icon_rgba_embedded_asset_decodes_to_128x128_rgba() {
    // The tray icon source is the embedded app-icon asset, not a procedural
    // glyph: decode must yield exactly 128x128 RGBA (Icon::from_rgba contract).
    let (rgba, width, height) = app_icon_rgba().expect("embedded app icon must decode");
    assert_eq!((width, height), (128, 128));
    assert_eq!(rgba.len(), 128 * 128 * 4);
}

#[test]
fn app_icon_rgba_embedded_asset_corners_are_transparent() {
    let (rgba, width, height) = app_icon_rgba().expect("embedded app icon must decode");
    let corners = [
        (0usize, 0usize),
        (width as usize - 1, 0),
        (0, height as usize - 1),
        (width as usize - 1, height as usize - 1),
    ];
    for (x, y) in corners {
        assert_eq!(
            pixel(&rgba, width as usize, x, y)[3],
            0,
            "corner {x},{y} must be transparent"
        );
    }
}

#[test]
fn app_icon_rgba_embedded_asset_has_opaque_pixels() {
    let (rgba, _width, _height) = app_icon_rgba().expect("embedded app icon must decode");
    assert!(
        rgba.as_chunks::<4>().0.iter().any(|p| p[3] == 255),
        "app icon must contain fully opaque pixels"
    );
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
