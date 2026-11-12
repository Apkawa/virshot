use super::*;

use crate::bindings::Action;
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
