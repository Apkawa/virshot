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
    assert_eq!(
        action_for_menu_id(&MenuId(MENU_ID_ABOUT.to_owned())),
        Some(TrayAction::About)
    );
    assert_eq!(action_for_menu_id(&MenuId("other".to_owned())), None);
}

#[test]
fn menu_labels_match_spec_section_4_5() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Menu labels render through catalog `tray.*`; the en values pin the
    // shipped English wording byte-for-byte (spec §4.5).
    assert_en_catalog(
        "tray.take_screenshot",
        &take_screenshot_label(),
        "Take screenshot",
    );
    assert_en_catalog("tray.settings", &settings_label(), "Settings");
    assert_en_catalog("tray.exit", &exit_label(), "Exit");
    assert_en_catalog("tray.about", &about_label(), "About");
}

#[test]
fn tray_menu_labels_render_through_catalog_keys() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The menu builder renders exactly the catalog quartet
    // (`tray.take_screenshot` / `tray.settings` / `tray.exit` /
    // `tray.about`) in menu order, and the tooltip renders through
    // `tray.tooltip` — spec §2: every tray string is a catalog lookup.
    let (take, settings, exit, about) = menu_labels();
    assert_eq!(take, rust_i18n::t!("tray.take_screenshot").as_ref());
    assert_eq!(settings, rust_i18n::t!("tray.settings").as_ref());
    assert_eq!(exit, rust_i18n::t!("tray.exit").as_ref());
    assert_eq!(about, rust_i18n::t!("tray.about").as_ref());
    assert_en_catalog("tray.tooltip", &tooltip(), "virshot");
}

#[test]
fn tray_menu_rebuild_uses_current_locale_labels() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Startup (`TrayController::new`) and the post-Apply rebuild
    // (`refresh_menu`) share ONE builder, so a locale change cannot drift the
    // two paths. The drift check covers the CONTRACTUAL items: muda
    // auto-numbers predefined item ids (the separator gets a fresh id per
    // build — verified in the locked muda source, and `action_for_menu_id`
    // maps no such id), so the separator is compared by its slot position
    // (no label), and the mapped quartet by id + label + order.
    let first = build_menu().expect("fresh items must append");
    let second = build_menu().expect("fresh items must append");
    let texts = |menu: &Menu| -> Vec<String> {
        menu.items()
            .iter()
            .map(|kind| {
                kind.as_menuitem()
                    .map(|item| item.text())
                    .unwrap_or_default()
            })
            .collect()
    };
    assert_eq!(texts(&first), texts(&second), "two builds must not drift");
    let ids: Vec<MenuId> = first
        .items()
        .iter()
        .filter_map(|kind| kind.as_menuitem().map(|item| item.id().clone()))
        .collect();
    assert_eq!(
        ids,
        vec![
            MenuId(MENU_ID_TAKE_SCREENSHOT.to_owned()),
            MenuId(MENU_ID_SETTINGS.to_owned()),
            MenuId(MENU_ID_EXIT.to_owned()),
            MenuId(MENU_ID_ABOUT.to_owned()),
        ],
        "mapped item ids must be stable across builds"
    );

    // The labeled items are exactly the catalog quartet in menu order — the
    // same seam `refresh_menu` renders after a locale switch.
    let labels: Vec<String> = first
        .items()
        .iter()
        .filter_map(|kind| kind.as_menuitem().map(|item| item.text()))
        .collect();
    let (take, settings, exit, about) = menu_labels();
    assert_eq!(
        labels,
        vec![take, settings, exit, about],
        "menu order must be take screenshot → settings → exit → about"
    );
}

/// Stage 3 (spec §2/§7): the tray menu seam that `refresh_menu` rebuilds on
/// Apply renders Russian labels under the `ru` locale. `ru` is restored
/// BEFORE the assertions so a failure never leaves it installed.
#[test]
fn tray_menu_labels_follow_russian_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let labels = menu_labels();

    rust_i18n::set_locale("en");

    assert_eq!(
        labels,
        (
            "Сделать снимок".to_owned(),
            "Настройки".to_owned(),
            "Выход".to_owned(),
            "О программе".to_owned(),
        ),
        "the tray menu must render Russian labels under the ru locale"
    );
}

/// Stage 4 (spec §2/§7): the tray menu seam renders the four new locales'
/// labels and must NOT stay on the English trio. `en` is restored BEFORE
/// each assertion.
#[test]
fn tray_menu_labels_render_in_each_locale() {
    let _guard = locale_guard();
    let english = (
        "Take screenshot".to_owned(),
        "Settings".to_owned(),
        "Exit".to_owned(),
        "About".to_owned(),
    );
    let expected: [(&str, [&str; 4]); 4] = [
        ("zh-CN", ["截屏", "设置", "退出", "关于"]),
        ("es", ["Capturar pantalla", "Ajustes", "Salir", "Acerca de"]),
        (
            "ja",
            ["スクリーンショットを撮る", "設定", "終了", "情報について"],
        ),
        ("ko", ["스크린샷 촬영", "설정", "종료", "정보"]),
    ];

    for (locale, labels) in expected {
        rust_i18n::set_locale(locale);
        let rendered = menu_labels();
        rust_i18n::set_locale("en");

        assert_ne!(
            rendered, english,
            "the tray menu must not stay English under {locale}, got {rendered:?}"
        );
        assert_eq!(
            rendered,
            (
                labels[0].to_owned(),
                labels[1].to_owned(),
                labels[2].to_owned(),
                labels[3].to_owned(),
            ),
            "the tray menu must render the {locale} catalog labels under {locale}"
        );
    }
}
